use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Cursor, Write},
    sync::Arc,
};

use sha2::{Digest, Sha256};

use camino::Utf8Component;
use flate2::{Compression, write::GzEncoder};
use icp::{
    Canister, InitArgs,
    canister::{build::Build, wasm},
    fs,
    manifest::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, EnvironmentManifest, Instructions,
        Item, LoadManifestFromPathError, ManagedMode, ManifestInitArgs, Mode, NetworkManifest,
        PROJECT_MANIFEST, ProjectManifest, SyncStep, SyncSteps,
        assets::DirField,
        load_manifest_from_path, plugin, prebuilt,
        prebuilt::{LocalSource, SourceField},
    },
    package::PackageCache,
    prelude::*,
    store_artifact,
};
use snafu::{ResultExt, Snafu};
use tar::Builder;

use crate::operations::{
    build::{BuildManyError, build_many_with_progress_bar},
    customize::CUSTOMIZE_FILE,
};

#[derive(Debug, Snafu)]
pub enum BundleError {
    #[snafu(display(
        "canister '{canister}' has a script sync step, which is not supported in bundles"
    ))]
    ScriptSyncStep { canister: String },

    #[snafu(display(
        "canister names {names:?} all sanitize to the same archive segment '{sanitized}'; \
         rename them to use distinct alphanumeric/-/_/. characters"
    ))]
    CanisterNameCollision {
        sanitized: String,
        names: Vec<String>,
    },

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

    #[snafu(display("failed to read '{path}'"))]
    ReadCustomize { path: PathBuf, source: fs::IoError },

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

    #[snafu(display("failed to canonicalize path '{path}'"))]
    CanonicalizePath {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display(
        "source path '{path}' for canister '{canister}' resolves outside the project directory \
         '{root}'; bundles cannot reference files outside the project"
    ))]
    SourceEscapesProject {
        canister: String,
        path: PathBuf,
        root: PathBuf,
    },

    #[snafu(display(
        "output path '{output}' is inside synced directory '{dir}'; bundling would include a \
         partial copy of the output file. Choose an output path outside this directory."
    ))]
    OutputOverlapsSyncDir { output: PathBuf, dir: PathBuf },

    #[snafu(display(
        "network '{network}' bind mount '{mount}' uses an absolute host path; \
         bundles require relative paths for portability"
    ))]
    AbsoluteBindMount { network: String, mount: String },

    #[snafu(display("failed to resolve plugin wasm for canister '{canister}'"))]
    ResolvePlugin {
        canister: String,
        source: wasm::WasmError,
    },

    #[snafu(display("failed to read plugin wasm for canister '{canister}'"))]
    ReadPlugin {
        canister: String,
        source: fs::IoError,
    },

    #[snafu(display("failed to read plugin file '{file}' for canister '{canister}'"))]
    ReadPluginFile {
        canister: String,
        file: String,
        source: fs::IoError,
    },
}

/// In-memory bytes destined for a single tar entry.
struct NamedBytes {
    archive_path: String,
    bytes: Vec<u8>,
}

/// On-disk directory to be recursively appended at `archive_prefix`.
struct DirEntry {
    src_path: PathBuf,
    archive_prefix: String,
}

/// Plugin input file. The canister/file metadata is carried so a read failure is attributable.
struct PluginFile {
    src_path: PathBuf,
    archive_path: String,
    canister_name: String,
    orig_file: String,
}

/// init_args file referenced from an environment manifest.
struct InitArgsFile {
    src_path: PathBuf,
    archive_path: String,
}

/// Everything the canister section contributes to the archive, separate from the manifest items.
#[derive(Default)]
struct BundleArtifacts {
    wasms: Vec<NamedBytes>,
    asset_dirs: Vec<DirEntry>,
    plugin_wasms: Vec<NamedBytes>,
    plugin_dirs: Vec<DirEntry>,
    plugin_files: Vec<PluginFile>,
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
    validate_canisters(&canisters)?;
    let canonical_project_dir = canonicalize(project_dir)?;
    let canonical_sync_dirs = validate_source_paths(&canisters, &canonical_project_dir)?;
    validate_output_path(output, &canonical_sync_dirs)?;

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

    let (canister_items, bundle_artifacts) =
        prepare_canisters(&canisters, &*artifacts, pkg_cache).await?;
    let networks = inline_networks(raw_manifest.networks, project_dir).await?;
    let (environments, init_args_files) = inline_environments(
        raw_manifest.environments,
        project_dir,
        &canonical_project_dir,
        &canisters,
    )
    .await?;

    let bundle_manifest = ProjectManifest {
        canisters: canister_items,
        networks,
        environments,
    };

    let customize_path = project_dir.join(CUSTOMIZE_FILE);
    let customize_bytes = match fs::read(&customize_path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(BundleError::ReadCustomize {
                path: customize_path,
                source,
            });
        }
    };

    write_archive(
        output,
        &bundle_manifest,
        customize_bytes.as_deref(),
        &bundle_artifacts,
        &init_args_files,
    )
}

/// Build the per-canister manifest items and collect the archive artifacts they reference.
async fn prepare_canisters(
    canisters: &[(PathBuf, Canister)],
    artifacts: &dyn store_artifact::Access,
    pkg_cache: &PackageCache,
) -> Result<(Vec<Item<CanisterManifest>>, BundleArtifacts), BundleError> {
    let mut items = Vec::with_capacity(canisters.len());
    let mut out = BundleArtifacts::default();
    for (canister_path, canister) in canisters {
        let item =
            prepare_canister(canister_path, canister, artifacts, pkg_cache, &mut out).await?;
        items.push(item);
    }
    Ok((items, out))
}

async fn prepare_canister(
    canister_path: &Path,
    canister: &Canister,
    artifacts: &dyn store_artifact::Access,
    pkg_cache: &PackageCache,
    out: &mut BundleArtifacts,
) -> Result<Item<CanisterManifest>, BundleError> {
    let path_name = path_segment(&canister.name);
    let wasm = artifacts
        .lookup(&canister.name)
        .await
        .context(LookupArtifactSnafu {
            canister: canister.name.clone(),
        })?;
    let sha256 = hex::encode(Sha256::digest(&wasm));
    let wasm_filename = format!("{path_name}.wasm");

    let mut bundle_sync_steps = Vec::with_capacity(canister.sync.steps.len());
    let mut plugin_idx: usize = 0;

    for step in &canister.sync.steps {
        match step {
            // validate_canisters rules this out
            SyncStep::Script(_) => unreachable!("validated by validate_canisters"),
            SyncStep::Assets(adapter) => {
                bundle_sync_steps.push(prepare_asset_step(adapter, canister_path, &path_name, out));
            }
            SyncStep::Plugin(adapter) => {
                let idx = plugin_idx;
                plugin_idx += 1;
                bundle_sync_steps.push(
                    prepare_plugin_step(
                        adapter,
                        canister,
                        canister_path,
                        &path_name,
                        idx,
                        pkg_cache,
                        out,
                    )
                    .await?,
                );
            }
        }
    }

    let sync = (!bundle_sync_steps.is_empty()).then_some(SyncSteps {
        steps: bundle_sync_steps,
    });

    out.wasms.push(NamedBytes {
        archive_path: wasm_filename.clone(),
        bytes: wasm,
    });

    Ok(Item::Manifest(CanisterManifest {
        name: canister.name.clone(),
        settings: canister.settings.clone(),
        init_args: canister.init_args.as_ref().map(convert_init_args),
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
    }))
}

fn prepare_asset_step(
    adapter: &icp::manifest::assets::Adapter,
    canister_path: &Path,
    path_name: &str,
    out: &mut BundleArtifacts,
) -> SyncStep {
    let dirs = adapter.dir.as_vec();
    let mut prefixed: Vec<String> = Vec::with_capacity(dirs.len());
    for d in &dirs {
        let archive_prefix = format!("{path_name}/{}", normalize_archive_dir(d));
        out.asset_dirs.push(DirEntry {
            src_path: canister_path.join(d),
            archive_prefix: archive_prefix.clone(),
        });
        prefixed.push(archive_prefix);
    }

    let new_dir = if prefixed.len() == 1 {
        DirField::Dir(prefixed.into_iter().next().unwrap())
    } else {
        DirField::Dirs(prefixed)
    };

    SyncStep::Assets(icp::manifest::assets::Adapter { dir: new_dir })
}

async fn prepare_plugin_step(
    adapter: &plugin::Adapter,
    canister: &Canister,
    canister_path: &Path,
    path_name: &str,
    idx: usize,
    pkg_cache: &PackageCache,
    out: &mut BundleArtifacts,
) -> Result<SyncStep, BundleError> {
    let plugin_wasm_path = format!("plugins/{path_name}/{idx}.wasm");

    let resolved = wasm::resolve(
        &adapter.source,
        canister_path,
        adapter.sha256.as_deref(),
        None,
        pkg_cache,
    )
    .await
    .context(ResolvePluginSnafu {
        canister: canister.name.clone(),
    })?;

    let plugin_bytes = fs::read(&resolved).context(ReadPluginSnafu {
        canister: canister.name.clone(),
    })?;
    let plugin_sha256 = hex::encode(Sha256::digest(&plugin_bytes));
    out.plugin_wasms.push(NamedBytes {
        archive_path: plugin_wasm_path.clone(),
        bytes: plugin_bytes,
    });

    // Plugin preopened dirs go under a `dirs/` subdir so a user-supplied dir literally named
    // `files` cannot collide with the `files/` area used for plugin input files.
    let bundle_dirs = adapter.dirs.as_ref().map(|dirs| {
        dirs.iter()
            .map(|d| {
                let archive_prefix = format!(
                    "plugins/{path_name}/{idx}/dirs/{}",
                    normalize_archive_dir(d)
                );
                out.plugin_dirs.push(DirEntry {
                    src_path: canister_path.join(d),
                    archive_prefix: archive_prefix.clone(),
                });
                archive_prefix
            })
            .collect::<Vec<_>>()
    });

    let bundle_files = adapter.files.as_ref().map(|files| {
        files
            .iter()
            .map(|f| {
                let archive_path = format!(
                    "plugins/{path_name}/{idx}/files/{}",
                    normalize_archive_dir(f)
                );
                out.plugin_files.push(PluginFile {
                    src_path: canister_path.join(f),
                    archive_path: archive_path.clone(),
                    canister_name: canister.name.clone(),
                    orig_file: f.clone(),
                });
                archive_path
            })
            .collect::<Vec<_>>()
    });

    Ok(SyncStep::Plugin(plugin::Adapter {
        source: SourceField::Local(LocalSource {
            path: plugin_wasm_path.as_str().into(),
        }),
        sha256: Some(plugin_sha256),
        dirs: bundle_dirs,
        files: bundle_files,
    }))
}

async fn inline_networks(
    items: Vec<Item<NetworkManifest>>,
    project_dir: &Path,
) -> Result<Vec<Item<NetworkManifest>>, BundleError> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
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
        if let Item::Manifest(ref net) = inlined {
            validate_network_for_bundle(net)?;
        }
        out.push(inlined);
    }
    Ok(out)
}

async fn inline_environments(
    items: Vec<Item<EnvironmentManifest>>,
    project_dir: &Path,
    canonical_project_dir: &Path,
    canisters: &[(PathBuf, Canister)],
) -> Result<(Vec<Item<EnvironmentManifest>>, Vec<InitArgsFile>), BundleError> {
    // Inline canisters resolve init_args paths relative to the project dir (matches the
    // Item::Manifest behavior in project.rs).
    let canister_path_map: HashMap<&str, &Path> = canisters
        .iter()
        .map(|(path, canister)| (canister.name.as_str(), path.as_path()))
        .collect();

    let mut out = Vec::with_capacity(items.len());
    let mut init_args_files = Vec::new();

    for item in items {
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
                    // Same containment rule as asset/plugin sources — a malicious manifest
                    // could otherwise point init_args at host files outside the project, and
                    // normalize_archive_dir would silently strip any leading `..` from the
                    // rewritten archive path so the escape wouldn't be visible there.
                    canonicalize_within_project(&src, canonical_project_dir, canister_name)?;
                    let archive_path = format!(
                        "init-args/{}/{}",
                        path_segment(canister_name),
                        normalize_archive_dir(orig_path)
                    );
                    init_args_files.push(InitArgsFile {
                        src_path: src,
                        archive_path: archive_path.clone(),
                    });
                    *mia = ManifestInitArgs::Path {
                        path: archive_path,
                        format: fmt.clone(),
                    };
                }
            }
        }

        out.push(inlined);
    }

    Ok((out, init_args_files))
}

fn write_archive(
    output: &Path,
    bundle_manifest: &ProjectManifest,
    customize_bytes: Option<&[u8]>,
    artifacts: &BundleArtifacts,
    init_args_files: &[InitArgsFile],
) -> Result<(), BundleError> {
    let manifest_yaml = serde_yaml::to_string(bundle_manifest).context(SerializeManifestSnafu)?;

    let file = File::create(output.as_std_path()).context(CreateOutputSnafu {
        path: output.to_path_buf(),
    })?;
    let gz = GzEncoder::new(BufWriter::new(file), Compression::default());
    let mut archive = Builder::new(gz);
    // Record symlinks as Symlink entries rather than slurping their targets — keeps secrets
    // outside the project from leaking via a symlinked asset dir.
    archive.follow_symlinks(false);
    // Strip mtime/uid/gid from headers written via append_dir_all so the archive is
    // byte-reproducible across machines.
    archive.mode(tar::HeaderMode::Deterministic);

    append_bytes(&mut archive, "icp.yaml", manifest_yaml.as_bytes())?;

    if let Some(customize_bytes) = customize_bytes {
        append_bytes(&mut archive, CUSTOMIZE_FILE, customize_bytes)?;
    }

    for nb in &artifacts.wasms {
        append_bytes(&mut archive, &nb.archive_path, &nb.bytes)?;
    }

    for entry in init_args_files {
        let data = fs::read(&entry.src_path).context(ReadInitArgsSnafu {
            path: entry.src_path.clone(),
        })?;
        append_bytes(&mut archive, &entry.archive_path, &data)?;
    }

    for d in &artifacts.asset_dirs {
        append_dir(&mut archive, &d.src_path, &d.archive_prefix)?;
    }

    for nb in &artifacts.plugin_wasms {
        append_bytes(&mut archive, &nb.archive_path, &nb.bytes)?;
    }

    for d in &artifacts.plugin_dirs {
        append_dir(&mut archive, &d.src_path, &d.archive_prefix)?;
    }

    for pf in &artifacts.plugin_files {
        let data = fs::read(&pf.src_path).context(ReadPluginFileSnafu {
            canister: pf.canister_name.clone(),
            file: pf.orig_file.clone(),
        })?;
        append_bytes(&mut archive, &pf.archive_path, &data)?;
    }

    // Finalize the tar trailer, the gzip trailer, and the underlying BufWriter — any of these
    // may fail to write the last bytes to disk, and we want to surface that.
    let gz = archive.into_inner().context(FlushArchiveSnafu)?;
    let buf = gz.finish().context(FlushArchiveSnafu)?;
    buf.into_inner().map_err(|e| BundleError::FlushArchive {
        source: e.into_error(),
    })?;

    Ok(())
}

fn append_bytes<W: Write>(
    archive: &mut Builder<W>,
    archive_path: &str,
    bytes: &[u8],
) -> Result<(), BundleError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, archive_path, Cursor::new(bytes))
        .context(WriteArchiveEntrySnafu {
            path: PathBuf::from(archive_path),
        })
}

fn append_dir<W: Write>(
    archive: &mut Builder<W>,
    src_path: &Path,
    archive_prefix: &str,
) -> Result<(), BundleError> {
    archive
        .append_dir_all(archive_prefix, src_path.as_std_path())
        .context(WriteArchiveEntrySnafu {
            path: PathBuf::from(archive_prefix),
        })
}

/// Up-front validation that the canister set can be bundled:
///  - no sync step is a script (we cannot replay an arbitrary shell command from the bundle)
///  - all sanitized canister names are unique (otherwise archive paths collide silently)
fn validate_canisters(canisters: &[(PathBuf, Canister)]) -> Result<(), BundleError> {
    for (_, canister) in canisters {
        for step in &canister.sync.steps {
            if matches!(step, SyncStep::Script(_)) {
                return ScriptSyncStepSnafu {
                    canister: canister.name.clone(),
                }
                .fail();
            }
        }
    }

    let mut by_segment: HashMap<String, Vec<String>> = HashMap::new();
    for (_, canister) in canisters {
        by_segment
            .entry(path_segment(&canister.name))
            .or_default()
            .push(canister.name.clone());
    }
    for (sanitized, mut names) in by_segment {
        if names.len() > 1 {
            names.sort();
            return CanisterNameCollisionSnafu { sanitized, names }.fail();
        }
    }

    Ok(())
}

/// Canonicalize every asset/plugin source path and confirm it lives inside the canonical
/// project directory. Returns the canonical sync-directory paths for use in output-overlap
/// detection.
fn validate_source_paths(
    canisters: &[(PathBuf, Canister)],
    canonical_project_dir: &Path,
) -> Result<Vec<PathBuf>, BundleError> {
    let mut canonical_sync_dirs = Vec::new();
    for (canister_path, canister) in canisters {
        for step in &canister.sync.steps {
            match step {
                SyncStep::Script(_) => {}
                SyncStep::Assets(adapter) => {
                    for d in adapter.dir.as_vec() {
                        let src = canister_path.join(&d);
                        let canon = canonicalize_within_project(
                            &src,
                            canonical_project_dir,
                            &canister.name,
                        )?;
                        canonical_sync_dirs.push(canon);
                    }
                }
                SyncStep::Plugin(adapter) => {
                    if let Some(dirs) = &adapter.dirs {
                        for d in dirs {
                            let src = canister_path.join(d);
                            let canon = canonicalize_within_project(
                                &src,
                                canonical_project_dir,
                                &canister.name,
                            )?;
                            canonical_sync_dirs.push(canon);
                        }
                    }
                    if let Some(files) = &adapter.files {
                        for f in files {
                            let src = canister_path.join(f);
                            canonicalize_within_project(
                                &src,
                                canonical_project_dir,
                                &canister.name,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(canonical_sync_dirs)
}

/// Refuse to write the bundle output into a directory we are about to recursively archive —
/// otherwise the partial bundle file would be included in itself.
fn validate_output_path(output: &Path, canonical_sync_dirs: &[PathBuf]) -> Result<(), BundleError> {
    let canonical_output = canonicalize_output(output)?;
    for sync_dir in canonical_sync_dirs {
        if canonical_output.starts_with(sync_dir) {
            return OutputOverlapsSyncDirSnafu {
                output: canonical_output,
                dir: sync_dir.clone(),
            }
            .fail();
        }
    }
    Ok(())
}

fn validate_network_for_bundle(net: &NetworkManifest) -> Result<(), BundleError> {
    let Mode::Managed(managed) = &net.configuration else {
        return Ok(());
    };
    let ManagedMode::Image {
        mounts: Some(mounts),
        ..
    } = managed.mode.as_ref()
    else {
        return Ok(());
    };
    for mount in mounts {
        if is_absolute_bind_mount_host(mount) {
            return AbsoluteBindMountSnafu {
                network: net.name.clone(),
                mount: mount.clone(),
            }
            .fail();
        }
    }
    Ok(())
}

/// Detects whether the host-path side of a bind mount (`host:container[:options]`) is absolute.
fn is_absolute_bind_mount_host(mount: &str) -> bool {
    let bytes = mount.as_bytes();
    // Drive-absolute Windows path (`C:\foo` / `C:/foo`). Detected before splitting so the
    // drive-letter colon isn't mistaken for the host/container separator. `C:foo` is
    // drive-*relative* and is left to the normal split below.
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }
    let host = mount.split(':').next().unwrap_or("");
    let h = host.as_bytes();
    !h.is_empty() && (h[0] == b'/' || h[0] == b'\\')
}

fn canonicalize(path: &Path) -> Result<PathBuf, BundleError> {
    path.canonicalize_utf8().context(CanonicalizePathSnafu {
        path: path.to_path_buf(),
    })
}

fn canonicalize_within_project(
    src: &Path,
    canonical_project_dir: &Path,
    canister: &str,
) -> Result<PathBuf, BundleError> {
    let canon = canonicalize(src)?;
    if !canon.starts_with(canonical_project_dir) {
        return SourceEscapesProjectSnafu {
            canister: canister.to_owned(),
            path: src.to_path_buf(),
            root: canonical_project_dir.to_path_buf(),
        }
        .fail();
    }
    Ok(canon)
}

/// Resolve the canonical form of an output path that may not exist yet. We canonicalize its
/// parent (which must exist before we can write a file there anyway) and append the filename.
fn canonicalize_output(output: &Path) -> Result<PathBuf, BundleError> {
    if output.exists() {
        return canonicalize(output);
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_str().is_empty())
        .unwrap_or(Path::new("."));
    let filename = output
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let canon_parent = canonicalize(parent)?;
    Ok(canon_parent.join(filename))
}

/// Normalizes a relative directory path for use as a tar archive prefix.
///
/// Resolves `.` and `..` lexically, strips leading `..` that would escape the
/// canister root, and discards any absolute prefix. The result is a clean
/// forward-slash-separated relative path safe to embed in a tar entry name.
/// Inputs that lexically resolve to the canister root (e.g. `.`, `tmp/..`)
/// return `.` so callers that build `format!("{prefix}/{normalized}")` produce
/// a well-formed path instead of a dangling trailing slash.
fn normalize_archive_dir(dir: &str) -> String {
    // Treat `\` as a path separator regardless of host OS so cross-platform bundles don't
    // produce archive entry names that decode as nested paths on Windows extraction.
    let dir = dir.replace('\\', "/");
    let mut parts: Vec<String> = Vec::new();
    for component in PathBuf::from(dir.as_str()).components() {
        match component {
            Utf8Component::Normal(s) => parts.push(s.to_owned()),
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                parts.pop();
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => parts.clear(),
        }
    }
    if parts.is_empty() {
        return ".".to_string();
    }
    parts.join("/")
}

/// Converts a canister name into a cross-platform-safe path segment.
///
/// Replaces any character that is not alphanumeric, `-`, `_`, or `.` with `_`.
/// This covers all characters prohibited on Windows (`< > : " / \ | ? *`),
/// path separators on Unix, and control characters. Additionally rewrites
/// Windows reserved device names (CON, PRN, AUX, NUL, COM0–COM9, LPT0–LPT9)
/// and trailing dots, which Windows strips and would otherwise produce
/// collisions or invalid filenames on extraction.
fn path_segment(name: &str) -> String {
    const RESERVED_WINDOWS_NAMES: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
        "LPT9",
    ];

    let mut s: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '_',
        })
        .collect();

    // Reserved device names apply to the stem (the part before the first `.`), regardless
    // of extension, and are matched case-insensitively.
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED_WINDOWS_NAMES.contains(&stem.as_str()) {
        s.insert(0, '_');
    }

    // Windows silently strips trailing dots from filenames, which would collide with a
    // sibling that has the dot stripped. Trailing spaces are already mapped to `_` above.
    if s.ends_with('.') {
        s.push('_');
    }

    s
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
