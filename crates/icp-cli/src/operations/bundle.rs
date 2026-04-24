use std::{
    fs::File,
    io::{BufWriter, Cursor},
    sync::Arc,
};

use sha2::{Digest, Sha256};

use flate2::{Compression, write::GzEncoder};
use icp::{
    Canister, InitArgs,
    canister::build::Build,
    manifest::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, Instructions, Item,
        LoadManifestFromPathError, ManifestInitArgs, PROJECT_MANIFEST, ProjectManifest, SyncStep,
        SyncSteps, assets::DirField, prebuilt,
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
        icp::manifest::load_manifest_from_path(&project_dir.join(PROJECT_MANIFEST))
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
        let wasm_filename = format!("{}.wasm", canister.name);

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
                        .map(|d| format!("{}/{d}", canister.name))
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
            asset_dirs.push((canister_path.clone(), canister.name.clone(), raw_asset_dirs));
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

    let bundle_manifest = ProjectManifest {
        canisters: bundle_canisters,
        networks: raw_manifest.networks,
        environments: raw_manifest.environments,
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

    // Asset directories
    for (canister_path, canister_name, dirs) in &asset_dirs {
        for dir in dirs {
            let src_path = canister_path.join(dir);
            let archive_prefix = format!("{canister_name}/{dir}");
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
