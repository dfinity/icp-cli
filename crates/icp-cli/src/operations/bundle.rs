use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Cursor},
    sync::Arc,
};

use sha2::{Digest, Sha256};

use camino::Utf8Component;
use flate2::{Compression, write::GzEncoder};
use icp::{
    Canister, InitArgs,
    canister::build::Build,
    fs,
    manifest::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, EnvironmentManifest, Instructions,
        Item, LoadManifestFromPathError, ManifestInitArgs, NetworkManifest, PROJECT_MANIFEST,
        ProjectManifest, SyncStep, SyncSteps, assets::DirField, load_manifest_from_path, prebuilt,
    },
    package::PackageCache,
    prelude::*,
    store_artifact,
};
use snafu::{ResultExt, Snafu};
use tar::Builder;

use crate::operations::build::{BuildManyError, build_many_with_progress_bar};

#[derive(Debug, Snafu)]
pub enum BundleError {
    #[snafu(display(
        "canister '{canister}' has a script sync step, which is not supported in bundles"
    ))]
    ScriptSyncStep { canister: String },

    #[snafu(transparent)]
    Build { source: BuildManyError },

    #[snafu(display("failed to look up built artifact for canister '{canister}'"))]
    LookupArtifact {
        canister: String,
        source: store_artifact::LookupArtifactError,
    },

    #[snafu(display("failed to load project manifest for bundle"))]
    LoadManifest { source: LoadManifestFromPathError },

    #[snafu(display("failed to load network manifest from '{path}'"))]
    LoadNetwork {
        path: PathBuf,
        source: LoadManifestFromPathError,
    },

    #[snafu(display("failed to load environment manifest from '{path}'"))]
    LoadEnvironment {
        path: PathBuf,
        source: LoadManifestFromPathError,
    },

    #[snafu(display("failed to read init_args file '{path}'"))]
    ReadInitArgs { path: PathBuf, source: fs::IoError },

    #[snafu(display("failed to serialize bundle manifest"))]
    SerializeManifest { source: serde_yaml::Error },

    #[snafu(display("failed to add '{path}' to bundle archive"))]
    WriteArchiveEntry {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("failed to create bundle output file at '{path}'"))]
    CreateOutput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("failed to finalize bundle archive"))]
    FlushArchive { source: std::io::Error },
}

pub(crate) async fn create_bundle(
    project_dir: &Path,
    canisters: Vec<(PathBuf, Canister)>,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn store_artifact::Access>,
    pkg_cache: &PackageCache,
    debug: bool,
    output: &Path,
) -> Result<(), BundleError> {
    for (_, canister) in &canisters {
        for step in &canister.sync.steps {
            if matches!(step, SyncStep::Script(_)) {
                return ScriptSyncStepSnafu {
                    canister: canister.name.clone(),
                }
                .fail();
            }
        }
    }

    build_many_with_progress_bar(
        canisters.clone(),
        builder,
        artifacts.clone(),
        pkg_cache,
        debug,
    )
    .await?;

    // Re-read the raw manifest to preserve networks and environments verbatim.
    let raw_manifest: ProjectManifest =
        load_manifest_from_path(&project_dir.join(PROJECT_MANIFEST))
            .await
            .context(LoadManifestSnafu)?;

    let mut bundle_canisters = Vec::new();
    let mut canister_wasms: Vec<(String, Vec<u8>)> = Vec::new();
    // (canister_path, canister_name, asset_dirs)
    let mut asset_dirs: Vec<(PathBuf, String, Vec<String>)> = Vec::new();

    for (canister_path, canister) in &canisters {
        let wasm = artifacts
            .lookup(&canister.name)
            .await
            .context(LookupArtifactSnafu {
                canister: canister.name.clone(),
            })?;

        let sha256 = hex::encode(Sha256::digest(&wasm));
        let path_name = path_segment(&canister.name);
        let wasm_filename = format!("{path_name}.wasm");

        // Collect asset dirs and rewrite their paths for the bundle.
        let mut bundle_sync_steps = Vec::new();
        let mut raw_asset_dirs = Vec::new();

        for step in &canister.sync.steps {
            match step {
                SyncStep::Script(_) => unreachable!("validated above"),
                SyncStep::Assets(adapter) => {
                    let dirs = adapter.dir.as_vec();
                    raw_asset_dirs.extend(dirs.clone());

                    let prefixed_dirs: Vec<String> = dirs
                        .iter()
                        .map(|d| format!("{path_name}/{}", normalize_archive_dir(d)))
                        .collect();

                    let new_dir = if prefixed_dirs.len() == 1 {
                        DirField::Dir(prefixed_dirs.into_iter().next().unwrap())
                    } else {
                        DirField::Dirs(prefixed_dirs)
                    };

                    bundle_sync_steps.push(SyncStep::Assets(icp::manifest::assets::Adapter {
                        dir: new_dir,
                    }));
                }
            }
        }

        if !raw_asset_dirs.is_empty() {
            asset_dirs.push((canister_path.clone(), path_name.clone(), raw_asset_dirs));
        }

        let init_args = canister.init_args.as_ref().map(convert_init_args);

        let sync = if bundle_sync_steps.is_empty() {
            None
        } else {
            Some(SyncSteps {
                steps: bundle_sync_steps,
            })
        };

        let canister_manifest = CanisterManifest {
            name: canister.name.clone(),
            settings: canister.settings.clone(),
            init_args,
            instructions: Instructions::BuildSync {
                build: BuildSteps {
                    steps: vec![BuildStep::Prebuilt(prebuilt::Adapter {
                        source: prebuilt::SourceField::Local(prebuilt::LocalSource {
                            path: wasm_filename.as_str().into(),
                        }),
                        sha256: Some(sha256),
                    })],
                },
                sync,
            },
        };

        bundle_canisters.push(Item::Manifest(canister_manifest));
        canister_wasms.push((wasm_filename, wasm));
    }

    let mut networks = Vec::new();
    for item in raw_manifest.networks {
        let inlined = match item {
            Item::Manifest(_) => item,
            Item::Path(ref path) => {
                let full = project_dir.join(path);
                let m = load_manifest_from_path::<NetworkManifest>(&full)
                    .await
                    .context(LoadNetworkSnafu { path: full })?;
                Item::Manifest(m)
            }
        };
        networks.push(inlined);
    }

    // canister name → its directory, for resolving init_args file references.
    // Inline canisters use the project dir (matching project.rs Item::Manifest handling).
    let canister_path_map: HashMap<&str, &Path> = canisters
        .iter()
        .map(|(path, canister)| (canister.name.as_str(), path.as_path()))
        .collect();

    // init_args files to copy into the archive: (source_path, archive_path).
    let mut init_args_files: Vec<(PathBuf, String)> = Vec::new();

    let mut environments = Vec::new();
    for item in raw_manifest.environments {
        let mut inlined = match item {
            Item::Manifest(_) => item,
            Item::Path(ref path) => {
                let full = project_dir.join(path);
                let m = load_manifest_from_path::<EnvironmentManifest>(&full)
                    .await
                    .context(LoadEnvironmentSnafu { path: full })?;
                Item::Manifest(m)
            }
        };

        if let Item::Manifest(ref mut env) = inlined
            && let Some(ref mut overrides) = env.init_args
        {
            for (canister_name, mia) in overrides.iter_mut() {
                if let ManifestInitArgs::Path {
                    path: orig_path,
                    format: fmt,
                } = &*mia
                {
                    let base = canister_path_map
                        .get(canister_name.as_str())
                        .copied()
                        .unwrap_or(project_dir);
                    let src = base.join(orig_path);
                    let archive_path = format!(
                        "init-args/{}/{}",
                        path_segment(canister_name),
                        normalize_archive_dir(orig_path)
                    );
                    init_args_files.push((src, archive_path.clone()));
                    *mia = ManifestInitArgs::Path {
                        path: archive_path,
                        format: fmt.clone(),
                    };
                }
            }
        }

        environments.push(inlined);
    }

    let bundle_manifest = ProjectManifest {
        canisters: bundle_canisters,
        networks,
        environments,
    };

    let manifest_yaml = serde_yaml::to_string(&bundle_manifest).context(SerializeManifestSnafu)?;

    let file = File::create(output.as_std_path()).context(CreateOutputSnafu {
        path: output.to_path_buf(),
    })?;
    let gz = GzEncoder::new(BufWriter::new(file), Compression::default());
    let mut archive = Builder::new(gz);

    // icp.yaml
    let manifest_bytes = manifest_yaml.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, "icp.yaml", Cursor::new(manifest_bytes))
        .context(WriteArchiveEntrySnafu {
            path: PathBuf::from("icp.yaml"),
        })?;

    // WASM files
    for (filename, wasm) in &canister_wasms {
        let mut header = tar::Header::new_gnu();
        header.set_size(wasm.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, filename, Cursor::new(wasm))
            .context(WriteArchiveEntrySnafu {
                path: PathBuf::from(filename),
            })?;
    }

    // Init args files
    for (src_path, archive_path) in &init_args_files {
        let data = fs::read(src_path).context(ReadInitArgsSnafu {
            path: src_path.clone(),
        })?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, archive_path, Cursor::new(data))
            .context(WriteArchiveEntrySnafu {
                path: PathBuf::from(archive_path),
            })?;
    }

    // Asset directories
    for (canister_path, canister_name, dirs) in &asset_dirs {
        for dir in dirs {
            let src_path = canister_path.join(dir);
            let archive_prefix = format!("{canister_name}/{}", normalize_archive_dir(dir));
            archive
                .append_dir_all(&archive_prefix, src_path.as_std_path())
                .context(WriteArchiveEntrySnafu {
                    path: PathBuf::from(&archive_prefix),
                })?;
        }
    }

    let gz = archive.into_inner().context(FlushArchiveSnafu)?;
    gz.finish().context(FlushArchiveSnafu)?;

    Ok(())
}

/// Normalizes a relative directory path for use as a tar archive prefix.
///
/// Resolves `.` and `..` lexically, strips leading `..` that would escape the
/// canister root, and discards any absolute prefix. The result is a clean
/// forward-slash-separated relative path safe to embed in a tar entry name.
fn normalize_archive_dir(dir: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in PathBuf::from(dir).components() {
        match component {
            Utf8Component::Normal(s) => parts.push(s.to_owned()),
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                parts.pop();
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => parts.clear(),
        }
    }
    parts.join("/")
}

/// Converts a canister name into a cross-platform-safe path segment.
///
/// Replaces any character that is not alphanumeric, `-`, `_`, or `.` with `_`.
/// This covers all characters prohibited on Windows (`< > : " / \ | ? *`),
/// path separators on Unix, and control characters.
fn path_segment(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '_',
        })
        .collect()
}

fn convert_init_args(args: &InitArgs) -> ManifestInitArgs {
    match args {
        InitArgs::Text { content, format } => ManifestInitArgs::Value {
            value: content.clone(),
            format: format.clone(),
        },
        InitArgs::Binary(bytes) => ManifestInitArgs::Value {
            value: hex::encode(bytes),
            format: ArgsFormat::Hex,
        },
    }
}
