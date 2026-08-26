use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufWriter, Cursor, Write},
    sync::Arc,
};

use sha2::{Digest, Sha256};

use crate::{
    Canister, CanisterArgs,
    canister::{ControllerRef, ManifestEnvVar, Settings, build::Build, wasm},
    fs,
    manifest::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, CanisterSelection, DependencyManifest,
        EnvironmentManifest, Instructions, Item, LoadManifestFromPathError, ManagedMode,
        ManifestArgs, Mode, NetworkManifest, PROJECT_MANIFEST, ProjectManifest, SyncStep,
        SyncSteps, load_manifest_from_path, plugin, prebuilt,
        prebuilt::{LocalSource, SourceField},
    },
    package::PackageCache,
    prelude::*,
    project::{WorkspaceInstance, WorkspaceInstancesError, workspace_instances},
    store_artifact,
};
use camino::Utf8Component;
use flate2::{Compression, write::GzEncoder};
use icp_sync_plugin::{covering_dirs, distinct_paths};
use snafu::{OptionExt, ResultExt, Snafu};
use tar::Builder;
use tracing::warn;

use icp_events::StepReporter;

use crate::operations::task::Reporter;

use crate::operations::build::{BuildManyError, build_many};

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
    Workspace { source: WorkspaceInstancesError },

    #[snafu(display(
        "dependency '{alias}' (path '{path}') resolves to '{dir}', which is outside the workspace \
         root '{root}'; bundling requires every dependency to live inside the workspace root, so \
         that the bundle is self-contained. Note that paths are resolved through symlinks, so a \
         symlinked dependency directory can land outside the root even when its path looks inside it"
    ))]
    DependencyOutsideWorkspace {
        alias: String,
        path: String,
        dir: PathBuf,
        root: PathBuf,
    },

    #[snafu(display(
        "canister '{canister}' does not belong to any project in the workspace; \
         its name does not match the workspace root or any of its dependencies"
    ))]
    OrphanCanister { canister: String },

    #[snafu(display(
        "two different files would be written to '{path}' in the bundle archive. \
         A dependency vendored in a directory that a canister's bundled artifacts \
         also occupy can cause this; vendor it elsewhere in the workspace"
    ))]
    DuplicateArchiveEntry { path: String },

    #[snafu(display(
        "the bundle archive would hold both the file '{parent}' and '{child}' beneath it, \
         which cannot be extracted. A dependency vendored in a directory that a canister's \
         bundled artifacts also occupy can cause this; vendor it elsewhere in the workspace"
    ))]
    NestedArchiveEntry { parent: String, child: String },

    #[snafu(display(
        "instance '{instance}' declares {declared} dependencies but {resolved} were resolved; \
         this is a bug in `icp project bundle`"
    ))]
    DependencyPrefixMismatch {
        instance: String,
        declared: usize,
        resolved: usize,
    },

    #[snafu(transparent)]
    Build { source: BuildManyError },

    #[snafu(display("failed to look up built artifact for canister '{canister}'"))]
    LookupArtifact {
        canister: String,
        source: store_artifact::LookupArtifactError,
    },

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

    #[snafu(display("failed to read args file '{path}'"))]
    ReadArgsFile { path: PathBuf, source: fs::IoError },

    #[snafu(display(
        "failed to read the file backing environment variable '{variable}' of canister '{canister}'"
    ))]
    ReadEnvVar {
        canister: String,
        variable: String,
        source: fs::IoError,
    },

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
        "the file '{path}' backing environment variable '{variable}' of canister '{canister}' \
         resolves outside the project directory '{root}'; bundles cannot reference files outside \
         the project"
    ))]
    EnvVarEscapesProject {
        canister: String,
        variable: String,
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

    #[snafu(display("failed to read app manifest '{path}'"))]
    ReadAppManifest { path: PathBuf, source: fs::IoError },

    #[snafu(display("failed to parse app manifest '{path}'"))]
    ParseAppManifest {
        path: PathBuf,
        source: serde_yaml::Error,
    },

    #[snafu(display("`images` in app manifest '{path}' must be a list of file paths"))]
    ImagesNotSequence { path: PathBuf },

    #[snafu(display("image entries in app manifest '{path}' must be file path strings"))]
    ImageNotString { path: PathBuf },

    #[snafu(display(
        "image path '{path}' resolves outside the project directory '{root}'; \
         bundles cannot reference files outside the project"
    ))]
    ImageEscapesProject { path: PathBuf, root: PathBuf },

    #[snafu(display(
        "images {paths:?} both map to the same bundle path 'images/{sanitized}'; \
         rename one so they use distinct file names"
    ))]
    ImageNameCollision {
        sanitized: String,
        paths: Vec<String>,
    },

    #[snafu(display("failed to serialize app manifest"))]
    SerializeAppManifest { source: serde_yaml::Error },

    #[snafu(display("failed to read image '{path}'"))]
    ReadImage { path: PathBuf, source: fs::IoError },
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

/// An `init_args` or `upgrade_args` file referenced from an environment manifest.
struct ArgsFile {
    src_path: PathBuf,
    archive_path: String,
}

/// One project manifest in the bundle: the root's at the archive root, plus one
/// per dependency instance at its workspace-relative directory.
struct InstanceManifest {
    archive_path: String,
    yaml: String,
}

/// The optional `icp_appmanifest.yaml` app-metadata file. We only understand its top-level
/// `images` list; all other keys are preserved semantically.
struct AppManifest {
    /// YAML to write at `APP_MANIFEST` in the archive. The original source text is used when
    /// no image relocation is needed; otherwise the YAML is re-serialized (formatting/comments may change).
    yaml: String,
    images: Vec<ImageFile>,
}

/// A image referenced from `icp_appmanifest.yaml`, relocated under `images/` in the bundle.
struct ImageFile {
    src_path: PathBuf,
    archive_path: String,
}

/// App-metadata manifest, included in bundles alongside the project manifest.
const APP_MANIFEST: &str = "icp_appmanifest.yaml";

/// Everything the canister section contributes to the archive, separate from the manifest items.
#[derive(Default)]
struct BundleArtifacts {
    wasms: Vec<NamedBytes>,
    plugin_wasms: Vec<NamedBytes>,
    plugin_dirs: Vec<DirEntry>,
    plugin_files: Vec<PluginFile>,
}

/// One project in the bundle: where it lives in the archive, its manifest as
/// written on disk, and the consolidated canisters it declares.
struct Instance {
    /// Archive directory for this instance — its workspace-relative path, and
    /// empty for the root project.
    prefix: String,

    /// The instance's directory on disk, the base for the relative paths in
    /// [`Self::manifest`].
    dir: PathBuf,

    manifest: ProjectManifest,

    /// Archive directory of the instance each `manifest.dependencies` entry
    /// resolves to, index-aligned with it.
    dependency_prefixes: Vec<String>,

    /// `(canister directory, consolidated canister)`, in workspace order. The
    /// canisters still carry their workspace store keys as names.
    canisters: Vec<(PathBuf, Canister)>,
}

/// The canisters the selected environment leaves out of the bundle, and the
/// lookup needed to recognize a manifest's reference to one of them.
///
/// A reference that survives into a bundled manifest names a canister that
/// manifest no longer declares, which the extracted bundle rejects at load.
struct Pruned<'a> {
    /// Store keys of the canisters the environment does not hold.
    dropped: &'a HashSet<String>,

    /// Each workspace instance's store-key prefix, by its canonical directory.
    prefixes_by_dir: &'a HashMap<PathBuf, String>,

    /// The environment the bundle is built for, for diagnostics.
    environment: &'a str,
}

impl Pruned<'_> {
    /// Whether a canister name written in one instance's manifest denotes a
    /// canister the environment leaves out.
    ///
    /// A name that resolves to no instance in the workspace is left alone: it is
    /// invalid, and reporting it is the manifest loader's job, not the bundler's.
    fn drops(&self, instance: &Instance, name: &str) -> bool {
        self.store_key(instance, name)
            .is_some_and(|key| self.dropped.contains(&key))
    }

    /// The workspace store key a name written in one instance's manifest refers
    /// to: either a bare local name, or `<relative path>:<canister>` naming a
    /// canister of a project that instance reaches through its dependencies. The
    /// path is the one the store key's own prefix is built from, so resolving it
    /// against the instance's directory gives that prefix back.
    fn store_key(&self, instance: &Instance, name: &str) -> Option<String> {
        let Some((rel, local)) = name.rsplit_once(':') else {
            return Some(override_store_key(&instance.prefix, name));
        };
        let dir = instance.dir.join(rel).canonicalize_utf8().ok()?;
        Some(override_store_key(self.prefixes_by_dir.get(&dir)?, local))
    }
}

pub async fn create_bundle(
    project_dir: &Path,
    canisters: Vec<(PathBuf, Canister)>,
    selected: &HashSet<String>,
    environment: &str,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn store_artifact::Access>,
    pkg_cache: &PackageCache,
    reporter: &Reporter,
    output: &Path,
) -> Result<(), BundleError> {
    // The bundle carries the canisters the selected environment holds and no
    // others: it is built for that environment, and a manifest that declared a
    // canister the archive has no wasm for could not be deployed from the
    // extraction.
    let (canisters, left_out): (Vec<_>, Vec<_>) = canisters
        .into_iter()
        .partition(|(_, canister)| selected.contains(&canister.name));
    let dropped: HashSet<String> = left_out
        .into_iter()
        .map(|(_, canister)| canister.name)
        .collect();

    // A bundle mirrors the workspace: the root project at the archive root and
    // each dependency instance at its workspace-relative directory, so the
    // dependency declarations — and the store keys and `PUBLIC_CANISTER_ID`
    // wiring deploy derives from them — carry over unchanged.
    let instances = group_canisters(
        workspace_instances(project_dir).await?,
        &canisters,
        project_dir,
    )?;
    validate_canisters(&instances)?;
    let mut prefixes_by_dir: HashMap<PathBuf, String> = HashMap::with_capacity(instances.len());
    for instance in &instances {
        prefixes_by_dir.insert(canonicalize(&instance.dir)?, instance.prefix.clone());
    }
    let pruned = Pruned {
        dropped: &dropped,
        prefixes_by_dir: &prefixes_by_dir,
        environment,
    };
    let canonical_project_dir = canonicalize(project_dir)?;
    let canonical_sync_dirs =
        validate_source_paths(project_dir, &canisters, &canonical_project_dir)?;
    validate_env_var_files(&canisters, &canonical_project_dir)?;
    validate_output_path(output, &canonical_sync_dirs)?;

    build_many(
        canisters.clone(),
        environment,
        builder,
        artifacts.clone(),
        pkg_cache,
        reporter,
    )
    .await?;

    // A root environment can override a dependency canister's install args, and
    // that path resolves against the *dependency's* directory, so the file's
    // location in the archive follows the canister's owning instance rather than
    // the manifest that declares the override.
    let canister_dirs: HashMap<&str, &Path> = canisters
        .iter()
        .map(|(path, canister)| (canister.name.as_str(), path.as_path()))
        .collect();
    let owner_prefixes: HashMap<&str, &str> = instances
        .iter()
        .flat_map(|instance| {
            instance
                .canisters
                .iter()
                .map(|(_, canister)| (canister.name.as_str(), instance.prefix.as_str()))
        })
        .collect();

    let mut bundle_artifacts = BundleArtifacts::default();
    let mut args_files: Vec<ArgsFile> = Vec::new();
    // Multiple environments can override the same canister's args from the same file,
    // which resolves to an identical archive path (and identical source). Emit each archive
    // entry once so we don't write duplicate tar headers for the same bytes.
    let mut seen_args_files: HashSet<String> = HashSet::new();
    let mut manifests: Vec<InstanceManifest> = Vec::with_capacity(instances.len());

    for instance in &instances {
        let canister_items = prepare_canisters(
            instance,
            &pruned,
            &*artifacts,
            pkg_cache,
            &mut bundle_artifacts,
        )
        .await?;
        let networks = inline_networks(&instance.manifest.networks, &instance.dir).await?;
        let environments = inline_environments(
            instance,
            &pruned,
            &canonical_project_dir,
            &canister_dirs,
            &owner_prefixes,
            &mut seen_args_files,
            &mut args_files,
        )
        .await?;

        let manifest = ProjectManifest {
            canisters: canister_items,
            dependencies: rewrite_dependencies(instance, &pruned)?,
            networks,
            environments,
        };

        manifests.push(InstanceManifest {
            archive_path: archive_join(&instance.prefix, PROJECT_MANIFEST),
            yaml: serde_yaml::to_string(&manifest).context(SerializeManifestSnafu)?,
        });
    }

    let app_manifest = prepare_app_manifest(project_dir, &canonical_project_dir)?;

    write_archive(
        output,
        &manifests,
        &bundle_artifacts,
        &args_files,
        app_manifest.as_ref(),
    )
}

/// The local name a canister's owning project knows it by: consolidation keys a
/// dependency canister as `<workspace-relative dir>:<local name>`, and a local
/// name never contains `:`.
fn local_name(store_key: &str) -> &str {
    match store_key.rsplit_once(':') {
        Some((_, local)) => local,
        None => store_key,
    }
}

/// The store-key prefix identifying a canister's owning instance — its
/// workspace-relative directory, and empty for the root project's own canisters.
fn store_key_prefix(store_key: &str) -> &str {
    match store_key.rsplit_once(':') {
        Some((prefix, _)) => prefix,
        None => "",
    }
}

/// Join an instance's archive directory with a path relative to that instance.
fn archive_join(prefix: &str, relative: &str) -> String {
    match prefix.is_empty() {
        true => relative.to_owned(),
        false => format!("{prefix}/{relative}"),
    }
}

/// The path from one instance's archive directory to another's. Both prefixes are
/// clean relative paths, so the result only rises to their common ancestor —
/// every directory it traverses is one the archive itself creates.
fn relative_archive_path(from: &str, to: &str) -> String {
    let split = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let from = split(from);
    let to = split(to);

    let shared = from
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - shared];
    parts.extend_from_slice(&to[shared..]);
    match parts.is_empty() {
        // Only reachable if an instance depended on itself, which consolidation
        // rejects as a cycle before bundling starts.
        true => ".".to_owned(),
        false => parts.join("/"),
    }
}

/// Point each dependency declaration at the instance's directory *in the
/// archive*.
///
/// The declared `path:` cannot be reused verbatim: it may be absolute, or reach
/// the instance through a symlink, in which case it does not describe where the
/// instance sits relative to the workspace root — and therefore not where it sits
/// in the archive either. For a plainly vendored layout the rewritten path is the
/// same path, modulo a leading `./`.
fn rewrite_dependencies(
    instance: &Instance,
    pruned: &Pruned<'_>,
) -> Result<Vec<DependencyManifest>, BundleError> {
    let declared = &instance.manifest.dependencies;
    let targets = &instance.dependency_prefixes;
    // `workspace_instances` resolves one prefix per declaration, in order.
    if declared.len() != targets.len() {
        return DependencyPrefixMismatchSnafu {
            instance: instance.prefix.clone(),
            declared: declared.len(),
            resolved: targets.len(),
        }
        .fail();
    }

    Ok(declared
        .iter()
        .zip(targets)
        .map(|(dep, target_prefix)| DependencyManifest {
            name: dep.name.clone(),
            path: relative_archive_path(&instance.prefix, target_prefix),
            // The exposure list names the dependency's own canisters, so a
            // left-out one is no longer there to expose.
            canisters: prune_selection(dep.canisters.clone(), |name| {
                pruned
                    .dropped
                    .contains(&override_store_key(target_prefix, name))
            }),
        })
        .collect())
}

/// Drop from a canister selection every name the environment leaves out. A list
/// emptied by the pruning becomes `CanisterSelection::None`, which is what an
/// empty list means once written to a manifest and read back.
fn prune_selection(
    selection: CanisterSelection,
    drops: impl Fn(&str) -> bool,
) -> CanisterSelection {
    let CanisterSelection::Named(mut names) = selection else {
        return selection;
    };
    names.retain(|name| !drops(name));
    match names.is_empty() {
        true => CanisterSelection::None,
        false => CanisterSelection::Named(names),
    }
}

/// Whether an instance's archive directory stays inside the workspace root.
///
/// The prefix is the instance's canonical directory relative to the canonical
/// workspace root, so a `..` component means the dependency lives outside the
/// workspace — including when a directory that looks inside the root is a
/// symlink pointing out of it. An absolute prefix means no relative path exists
/// at all (a different Windows drive).
fn prefix_is_inside_workspace(prefix: &str) -> bool {
    Path::new(prefix)
        .components()
        .all(|c| matches!(c, Utf8Component::Normal(_) | Utf8Component::CurDir))
}

/// Attach each consolidated canister to the instance that declares it, rejecting
/// any dependency that resolves outside the workspace root.
fn group_canisters(
    instances: Vec<WorkspaceInstance>,
    canisters: &[(PathBuf, Canister)],
    project_dir: &Path,
) -> Result<Vec<Instance>, BundleError> {
    let mut out: Vec<Instance> = Vec::with_capacity(instances.len());
    let mut by_prefix: HashMap<String, usize> = HashMap::with_capacity(instances.len());

    for instance in instances {
        if !prefix_is_inside_workspace(&instance.prefix) {
            let (alias, path) = instance.declared_as.unwrap_or_default();
            return DependencyOutsideWorkspaceSnafu {
                alias,
                path,
                dir: instance.dir,
                root: project_dir.to_path_buf(),
            }
            .fail();
        }

        by_prefix.insert(instance.prefix.clone(), out.len());
        out.push(Instance {
            prefix: instance.prefix,
            dir: instance.dir,
            manifest: instance.manifest,
            dependency_prefixes: instance.dependency_prefixes,
            canisters: Vec::new(),
        });
    }

    for (canister_dir, canister) in canisters {
        let index =
            by_prefix
                .get(store_key_prefix(&canister.name))
                .context(OrphanCanisterSnafu {
                    canister: canister.name.clone(),
                })?;
        out[*index]
            .canisters
            .push((canister_dir.clone(), canister.clone()));
    }

    Ok(out)
}

/// Build one instance's manifest items and collect the archive artifacts they reference.
async fn prepare_canisters(
    instance: &Instance,
    pruned: &Pruned<'_>,
    artifacts: &dyn store_artifact::Access,
    pkg_cache: &PackageCache,
    out: &mut BundleArtifacts,
) -> Result<Vec<Item<CanisterManifest>>, BundleError> {
    // Store key -> local name, for rewriting controller references back to the
    // names this instance's own manifest uses.
    let local_names: HashMap<&str, &str> = instance
        .canisters
        .iter()
        .map(|(_, canister)| (canister.name.as_str(), local_name(&canister.name)))
        .collect();

    let mut items = Vec::with_capacity(instance.canisters.len());
    for (canister_path, canister) in &instance.canisters {
        let item = prepare_canister(
            &instance.prefix,
            canister_path,
            canister,
            &local_names,
            pruned,
            artifacts,
            pkg_cache,
            out,
        )
        .await?;
        items.push(item);
    }
    Ok(items)
}

#[allow(clippy::too_many_arguments)]
async fn prepare_canister(
    prefix: &str,
    canister_path: &Path,
    canister: &Canister,
    local_names: &HashMap<&str, &str>,
    pruned: &Pruned<'_>,
    artifacts: &dyn store_artifact::Access,
    pkg_cache: &PackageCache,
    out: &mut BundleArtifacts,
) -> Result<Item<CanisterManifest>, BundleError> {
    let local = local_name(&canister.name);
    let path_name = path_segment(local);
    let wasm = artifacts
        .lookup(&canister.name)
        .await
        .context(LookupArtifactSnafu {
            canister: canister.name.clone(),
        })?;
    let sha256 = hex::encode(Sha256::digest(&wasm));
    // Manifest paths are relative to the instance that declares the canister;
    // archive paths carry the instance's directory as well.
    let wasm_filename = format!("canisters/{path_name}.wasm");

    let mut bundle_sync_steps = Vec::with_capacity(canister.sync.steps.len());
    let mut plugin_idx: usize = 0;

    for step in &canister.sync.steps {
        match step {
            // validate_canisters rules this out up front; return the same error rather than
            // panicking if that invariant is ever bypassed.
            SyncStep::Script(_) => {
                return ScriptSyncStepSnafu {
                    canister: canister.name.clone(),
                }
                .fail();
            }
            SyncStep::Plugin(adapter) => {
                let idx = plugin_idx;
                plugin_idx += 1;
                bundle_sync_steps.push(
                    prepare_plugin_step(
                        adapter,
                        prefix,
                        canister,
                        canister_path,
                        &path_name,
                        idx,
                        local_names,
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
        archive_path: archive_join(prefix, &wasm_filename),
        bytes: wasm,
    });

    Ok(Item::Manifest(CanisterManifest {
        name: local.to_owned(),
        settings: localize_controllers(
            canister.settings.clone().into(),
            &canister.name,
            local_names,
            pruned,
        ),
        init_args: canister.init_args.as_ref().map(convert_args),
        upgrade_args: canister.upgrade_args.as_ref().map(convert_args),
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

/// Rewrite controller references from workspace store keys back to the local
/// names of the instance being written, dropping the ones the selected
/// environment leaves out of the bundle.
///
/// Consolidation translates a dependency's references to its own siblings into
/// store keys, which contain `:` and so are not valid canister names. References
/// that are not one of this instance's store keys are left alone: they are
/// already plain names that resolved against the workspace, and they resolve the
/// same way from the bundle.
fn localize_controllers<EnvVar>(
    mut settings: Settings<EnvVar>,
    canister: &str,
    local_names: &HashMap<&str, &str>,
    pruned: &Pruned<'_>,
) -> Settings<EnvVar> {
    if let Some(controllers) = &mut settings.controllers {
        // A reference consolidation has already resolved is spelled as the store
        // key it resolved to, so the left-out keys are what to match against.
        controllers.retain(|cref| match cref {
            ControllerRef::CanisterName(name) if pruned.dropped.contains(name.as_str()) => {
                warn!(
                    "Canister '{canister}' names '{name}' as a controller, which environment \
                     '{}' does not contain; the bundle drops the reference.",
                    pruned.environment,
                );
                false
            }
            _ => true,
        });
        for cref in controllers.iter_mut() {
            if let ControllerRef::CanisterName(name) = cref
                && let Some(local) = local_names.get(name.as_str())
            {
                *name = (*local).to_owned();
            }
        }
    }
    settings
}

/// Rewrite a plugin's declared call targets from workspace store keys back to the
/// local names of the instance being written, on the same grounds as
/// [`localize_controllers`].
fn localize_call_targets(
    canisters: Option<&[String]>,
    local_names: &HashMap<&str, &str>,
) -> Option<Vec<String>> {
    canisters.map(|canisters| {
        canisters
            .iter()
            .map(|target| match local_names.get(target.as_str()) {
                Some(local) => (*local).to_owned(),
                None => target.clone(),
            })
            .collect()
    })
}

#[allow(clippy::too_many_arguments)]
async fn prepare_plugin_step(
    adapter: &plugin::Adapter,
    prefix: &str,
    canister: &Canister,
    canister_path: &Path,
    path_name: &str,
    idx: usize,
    local_names: &HashMap<&str, &str>,
    pkg_cache: &PackageCache,
    out: &mut BundleArtifacts,
) -> Result<SyncStep, BundleError> {
    let plugin_wasm_path = format!("plugins/{path_name}/{idx}.wasm");

    let resolved = wasm::resolve(
        &adapter.source,
        canister_path,
        adapter.sha256.as_deref(),
        &StepReporter::null(),
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
        archive_path: archive_join(prefix, &plugin_wasm_path),
        bytes: plugin_bytes,
    });

    // A `dirs:` entry (which only an `icp:sync-plugin@0.1` plugin takes) goes under a `dirs/`
    // subdir so a user-supplied dir literally named `files` cannot collide with the `files/`
    // area the `files:` entries occupy. The declared paths are rewritten to their archive
    // locations; each entry's map key is carried through unchanged.
    let dirs_prefix = format!("plugins/{path_name}/{idx}/dirs");
    let files_prefix = format!("plugins/{path_name}/{idx}/files");
    let bundle_dirs = adapter
        .dirs
        .as_ref()
        .map(|dirs| dirs.map_paths(|dir| format!("{dirs_prefix}/{}", normalize_archive_dir(dir))));
    let bundle_files = adapter.files.as_ref().map(|files| {
        files.map_paths(|file| format!("{files_prefix}/{}", normalize_archive_dir(file)))
    });

    // The rewritten manifest above keeps every declared entry; the archive holds
    // the trees and files behind them, of which there are fewer. A directory
    // named under two keys is one tree to copy, and a declared subdirectory of
    // another is already inside its copy — writing either twice would collide in
    // the archive. The reduction runs over the paths as declared, so two that
    // only *look* alike once rewritten (`../shared` and `shared` both normalize
    // to `shared`) stay separate and are still caught as a collision.
    let declared = |paths: &Option<plugin::NamedPaths>| -> Vec<String> {
        paths
            .iter()
            .flat_map(plugin::NamedPaths::entries)
            .map(|entry| entry.path.to_string())
            .collect()
    };
    // A `files:` entry names a directory or a file, and which it is comes from what is on
    // disk — the same rule the plugin host applies. So partition on that before deciding
    // whether the archive gets a tree or a single file.
    let (file_dirs, file_files): (Vec<String>, Vec<String>) = declared(&adapter.files)
        .into_iter()
        .partition(|path| canister_path.join(path).is_dir());

    for (dir, dir_prefix) in covering_dirs(declared(&adapter.dirs).iter().map(String::as_str))
        .into_iter()
        .map(|dir| (dir, &dirs_prefix))
        .chain(
            covering_dirs(file_dirs.iter().map(String::as_str))
                .into_iter()
                .map(|dir| (dir, &files_prefix)),
        )
    {
        out.plugin_dirs.push(DirEntry {
            src_path: canister_path.join(dir),
            archive_prefix: archive_join(
                prefix,
                &format!("{dir_prefix}/{}", normalize_archive_dir(dir)),
            ),
        });
    }
    for file in distinct_paths(file_files.iter().map(String::as_str)) {
        out.plugin_files.push(PluginFile {
            src_path: canister_path.join(file),
            archive_path: archive_join(
                prefix,
                &format!("{files_prefix}/{}", normalize_archive_dir(file)),
            ),
            canister_name: canister.name.clone(),
            orig_file: file.to_string(),
        });
    }

    Ok(SyncStep::Plugin(Box::new(plugin::Adapter {
        source: SourceField::Local(LocalSource {
            path: plugin_wasm_path.as_str().into(),
        }),
        sha256: Some(plugin_sha256),
        dirs: bundle_dirs,
        files: bundle_files,
        canisters: localize_call_targets(adapter.canisters.as_deref(), local_names),
        fields: adapter.fields.clone(),
    })))
}

async fn inline_networks(
    items: &[Item<NetworkManifest>],
    instance_dir: &Path,
) -> Result<Vec<Item<NetworkManifest>>, BundleError> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let inlined = match item {
            Item::Manifest(_) => item.clone(),
            Item::Path(path) => {
                let full = instance_dir.join(path);
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

/// The store key an environment override's canister name refers to. A member's
/// manifest names its own canisters locally (`registry`), while the root's names
/// theirs by store key (`vendor/openemail:registry`); prefixing with the
/// declaring instance turns either spelling into the store key the workspace-wide
/// maps are keyed by.
fn override_store_key(instance_prefix: &str, canister_name: &str) -> String {
    match instance_prefix.is_empty() {
        true => canister_name.to_owned(),
        false => format!("{instance_prefix}:{canister_name}"),
    }
}

/// The directory an environment override resolves its file references against —
/// both `init_args` and the file backing an environment variable. It is the
/// referenced canister's own directory, which for a dependency canister is its
/// own project rather than the manifest declaring the override.
fn override_base_dir<'a>(
    store_key: &str,
    canister_dirs: &HashMap<&str, &'a Path>,
    instance_dir: &'a Path,
) -> &'a Path {
    canister_dirs
        .get(store_key)
        .copied()
        .unwrap_or(instance_dir)
}

/// Archive directory holding the files an environment's `init_args` overrides
/// point at, and the one for its `upgrade_args` overrides. Kept apart so the
/// same canister can override both from same-named files.
const INIT_ARGS_DIR: &str = "init-args";
const UPGRADE_ARGS_DIR: &str = "upgrade-args";

/// Relocate the files one environment's args overrides point at into
/// `archive_dir`, rewriting each override to name the archived copy.
#[allow(clippy::too_many_arguments)]
fn relocate_args_overrides(
    overrides: &mut HashMap<String, ManifestArgs>,
    archive_dir: &str,
    instance_prefix: &str,
    instance_dir: &Path,
    canonical_project_dir: &Path,
    canister_dirs: &HashMap<&str, &Path>,
    owner_prefixes: &HashMap<&str, &str>,
    seen_archive_paths: &mut HashSet<String>,
    args_files: &mut Vec<ArgsFile>,
) -> Result<(), BundleError> {
    for (canister_name, ma) in overrides.iter_mut() {
        let ManifestArgs::Path {
            path: orig_path,
            format: fmt,
        } = &*ma
        else {
            continue;
        };
        let store_key = override_store_key(instance_prefix, canister_name);
        let base = override_base_dir(&store_key, canister_dirs, instance_dir);
        let src = base.join(orig_path);
        // Same containment rule as asset/plugin sources — a malicious manifest
        // could otherwise point the args at host files outside the project, and
        // normalize_archive_dir would silently strip any leading `..` from the
        // rewritten archive path so the escape wouldn't be visible there.
        canonicalize_within_project(&src, canonical_project_dir, canister_name)?;
        let manifest_path = format!(
            "{archive_dir}/{}/{}",
            path_segment(canister_name),
            normalize_archive_dir(orig_path)
        );
        // The reference is resolved against the canister's directory, so the
        // file has to be archived under the *canister's* instance — which is not
        // the declaring instance when the root overrides a dependency's canister.
        let owner_prefix = owner_prefixes
            .get(store_key.as_str())
            .copied()
            .unwrap_or(instance_prefix);
        let archive_path = archive_join(owner_prefix, &manifest_path);
        if seen_archive_paths.insert(archive_path.clone()) {
            args_files.push(ArgsFile {
                src_path: src,
                archive_path,
            });
        }
        *ma = ManifestArgs::Path {
            path: manifest_path,
            format: fmt.clone(),
        };
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn inline_environments(
    instance: &Instance,
    pruned: &Pruned<'_>,
    canonical_project_dir: &Path,
    canister_dirs: &HashMap<&str, &Path>,
    owner_prefixes: &HashMap<&str, &str>,
    seen_archive_paths: &mut HashSet<String>,
    args_files: &mut Vec<ArgsFile>,
) -> Result<Vec<Item<EnvironmentManifest>>, BundleError> {
    let items = &instance.manifest.environments;
    let instance_prefix = instance.prefix.as_str();
    let instance_dir = instance.dir.as_path();
    let mut out = Vec::with_capacity(items.len());

    for item in items {
        let mut inlined = match item {
            Item::Manifest(_) => item.clone(),
            Item::Path(path) => {
                let full = instance_dir.join(path);
                let m = load_manifest_from_path::<EnvironmentManifest>(&full)
                    .await
                    .context(LoadEnvironmentSnafu { path: full })?;
                Item::Manifest(m)
            }
        };

        // Before the overrides below are followed to the files they name: an
        // override for a left-out canister resolves its paths against that
        // canister's directory, which the bundle no longer knows.
        if let Item::Manifest(ref mut env) = inlined {
            prune_environment(env, instance, pruned);
        }

        if let Item::Manifest(ref mut env) = inlined {
            for (overrides, archive_dir) in [
                (env.init_args.as_mut(), INIT_ARGS_DIR),
                (env.upgrade_args.as_mut(), UPGRADE_ARGS_DIR),
            ] {
                let Some(overrides) = overrides else { continue };
                relocate_args_overrides(
                    overrides,
                    archive_dir,
                    instance_prefix,
                    instance_dir,
                    canonical_project_dir,
                    canister_dirs,
                    owner_prefixes,
                    seen_archive_paths,
                    args_files,
                )?;
            }
        }

        // File-backed environment variable values are read and written into the
        // bundled manifest inline, the same as the ones a canister manifest
        // declares (consolidation resolves those before bundling), so the file
        // itself does not travel with the bundle.
        if let Item::Manifest(ref mut env) = inlined
            && let Some(ref mut overrides) = env.settings
        {
            for (canister_name, settings) in overrides.iter_mut() {
                let Some(vars) = settings.environment_variables.as_mut() else {
                    continue;
                };
                let store_key = override_store_key(instance_prefix, canister_name);
                let base = override_base_dir(&store_key, canister_dirs, instance_dir);
                for (variable, var) in vars.iter_mut() {
                    if let ManifestEnvVar::Path { path } = &*var {
                        let src = base.join(path);
                        // Same containment rule as an init_args override above: a
                        // manifest must not have the bundle carry off a file from
                        // outside the project, inlined value or archived copy.
                        let canon = canonicalize(&src)?;
                        if !canon.starts_with(canonical_project_dir) {
                            return EnvVarEscapesProjectSnafu {
                                canister: canister_name.clone(),
                                variable: variable.clone(),
                                path: src,
                                root: canonical_project_dir.to_path_buf(),
                            }
                            .fail();
                        }
                        let value = fs::read_to_string(&src).context(ReadEnvVarSnafu {
                            canister: canister_name,
                            variable,
                        })?;
                        *var = ManifestEnvVar::Value(value.trim().to_owned());
                    }
                }
            }
        }

        out.push(inlined);
    }

    Ok(out)
}

/// Drop from one environment every reference to a canister the selected
/// environment leaves out of the bundle: the canisters it lists, the
/// per-canister settings and init_args it overrides, and the controllers those
/// settings name.
///
/// The environment being pruned is not necessarily the one the bundle was built
/// for — a bundle keeps every environment its manifests declare, and each of
/// them can only ever hold canisters the bundle carries.
fn prune_environment(env: &mut EnvironmentManifest, instance: &Instance, pruned: &Pruned<'_>) {
    env.canisters = prune_selection(std::mem::take(&mut env.canisters), |name| {
        pruned.drops(instance, name)
    });
    if let Some(settings) = &mut env.settings {
        settings.retain(|name, _| !pruned.drops(instance, name));
        // An override's own controller list survives the pruning above, which
        // only reaches the canister an override configures: a kept canister can
        // still be handed a controller the bundle does not carry.
        for (canister, overrides) in settings.iter_mut() {
            let Some(controllers) = &mut overrides.controllers else {
                continue;
            };
            controllers.retain(|cref| match cref {
                ControllerRef::CanisterName(name) if pruned.drops(instance, name) => {
                    warn!(
                        "Environment '{}' names '{name}' as a controller of '{canister}', which \
                         environment '{}' does not contain; the bundle drops the reference.",
                        env.name, pruned.environment,
                    );
                    false
                }
                _ => true,
            });
        }
    }
    if let Some(init_args) = &mut env.init_args {
        init_args.retain(|name, _| !pruned.drops(instance, name));
    }
    if let Some(upgrade_args) = &mut env.upgrade_args {
        upgrade_args.retain(|name, _| !pruned.drops(instance, name));
    }
}

/// Load `icp_appmanifest.yaml` if present, rewriting its top-level `images` paths to point at
/// copies relocated under `images/` in the bundle. Returns `None` when the file is absent.
fn prepare_app_manifest(
    project_dir: &Path,
    canonical_project_dir: &Path,
) -> Result<Option<AppManifest>, BundleError> {
    let manifest_path = project_dir.join(APP_MANIFEST);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&manifest_path).context(ReadAppManifestSnafu {
        path: &manifest_path,
    })?;
    let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw).context(ParseAppManifestSnafu {
        path: &manifest_path,
    })?;

    let Some(images_val) = doc.get_mut("images") else {
        // No images to relocate; embed the file unchanged.
        return Ok(Some(AppManifest {
            yaml: raw,
            images: Vec::new(),
        }));
    };
    let seq = images_val
        .as_sequence_mut()
        .context(ImagesNotSequenceSnafu {
            path: &manifest_path,
        })?;

    let mut images = Vec::with_capacity(seq.len());
    // Maps a relocated archive name back to the canonical source and original path it came from,
    // so identical entries are deduplicated and distinct sources that flatten to the same name
    // are reported as a collision.
    let mut seen: HashMap<String, (PathBuf, String)> = HashMap::new();
    for entry in seq.iter_mut() {
        let orig = entry
            .as_str()
            .context(ImageNotStringSnafu {
                path: &manifest_path,
            })?
            .to_owned();
        let src = project_dir.join(&orig);
        let canon = canonicalize(&src)?;
        if !canon.starts_with(canonical_project_dir) {
            return ImageEscapesProjectSnafu {
                path: src,
                root: canonical_project_dir.to_path_buf(),
            }
            .fail();
        }

        // Flatten into the top-level `images/` folder by basename, sanitized the same way
        // canister name segments are.
        let base = canon.file_name().unwrap_or(orig.as_str());
        let sanitized = path_segment(base);
        let archive_path = format!("images/{sanitized}");

        match seen.get(&sanitized) {
            Some((prev_canon, _)) if *prev_canon == canon => {}
            Some((_, prev_orig)) => {
                let mut paths = vec![prev_orig.clone(), orig.clone()];
                paths.sort();
                return ImageNameCollisionSnafu { sanitized, paths }.fail();
            }
            None => {
                seen.insert(sanitized.clone(), (canon.clone(), orig.clone()));
                images.push(ImageFile {
                    src_path: canon,
                    archive_path: archive_path.clone(),
                });
            }
        }

        *entry = serde_yaml::Value::String(archive_path);
    }

    let yaml = serde_yaml::to_string(&doc).context(SerializeAppManifestSnafu)?;
    Ok(Some(AppManifest { yaml, images }))
}

/// Reject an entry layout the archive cannot represent.
///
/// Instance directories are workspace-relative, so a dependency vendored at, say,
/// `canisters/` shares an archive directory with the root's wasms. Two entries at
/// the same path would be silently resolved by whichever is extracted last, and an
/// entry *nested under* another — a wasm at `canisters/app.wasm` alongside a
/// manifest at `canisters/app.wasm/icp.yaml` — would need one path to be both a
/// file and a directory. Recursively appended directories are covered by the same
/// rule: everything they contribute lives under the prefix checked here, so a
/// nested entry is exactly what would collide with their contents.
///
/// Checked before the output file is created, so a rejected bundle leaves nothing
/// behind.
fn validate_archive_paths(paths: &[&str]) -> Result<(), BundleError> {
    let entries: HashSet<&str> = HashSet::from_iter(paths.iter().copied());
    if entries.len() != paths.len() {
        let mut seen: HashSet<&str> = HashSet::new();
        let duplicate = paths
            .iter()
            .find(|p| !seen.insert(p))
            .expect("a duplicate exists when the set is smaller than the list");
        return DuplicateArchiveEntrySnafu {
            path: (*duplicate).to_owned(),
        }
        .fail();
    }

    for path in paths {
        // Every proper ancestor of an entry must not itself be an entry.
        let mut ancestor = *path;
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if entries.contains(parent) {
                return NestedArchiveEntrySnafu {
                    parent: parent.to_owned(),
                    child: (*path).to_owned(),
                }
                .fail();
            }
            ancestor = parent;
        }
    }

    Ok(())
}

/// A tar builder for the bundle's entries. Paths are validated by
/// [`validate_archive_paths`] before any of this runs.
struct ArchiveWriter<W: Write> {
    builder: Builder<W>,
}

impl<W: Write> ArchiveWriter<W> {
    fn new(inner: W) -> Self {
        let mut builder = Builder::new(inner);
        // Record symlinks as Symlink entries rather than slurping their targets — keeps secrets
        // outside the project from leaking via a symlinked asset dir.
        builder.follow_symlinks(false);
        // Strip mtime/uid/gid from entry headers so they are metadata-normalized across machines.
        // Note this does not make the archive fully byte-reproducible: `append_dir` relies on
        // `append_dir_all`, which walks `read_dir` in the filesystem's order, so entry ordering
        // within a directory can still differ between machines.
        builder.mode(tar::HeaderMode::Deterministic);
        Self { builder }
    }

    fn bytes(&mut self, archive_path: &str, bytes: &[u8]) -> Result<(), BundleError> {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        self.builder
            .append_data(&mut header, archive_path, Cursor::new(bytes))
            .context(WriteArchiveEntrySnafu {
                path: PathBuf::from(archive_path),
            })
    }

    fn dir(&mut self, src_path: &Path, archive_prefix: &str) -> Result<(), BundleError> {
        self.builder
            .append_dir_all(archive_prefix, src_path.as_std_path())
            .context(WriteArchiveEntrySnafu {
                path: PathBuf::from(archive_prefix),
            })
    }

    fn into_inner(self) -> Result<W, BundleError> {
        self.builder.into_inner().context(FlushArchiveSnafu)
    }
}

fn write_archive(
    output: &Path,
    manifests: &[InstanceManifest],
    artifacts: &BundleArtifacts,
    args_files: &[ArgsFile],
    app_manifest: Option<&AppManifest>,
) -> Result<(), BundleError> {
    let paths: Vec<&str> = manifests
        .iter()
        .map(|m| m.archive_path.as_str())
        .chain(app_manifest.iter().flat_map(|app| {
            std::iter::once(APP_MANIFEST).chain(app.images.iter().map(|i| i.archive_path.as_str()))
        }))
        .chain(artifacts.wasms.iter().map(|nb| nb.archive_path.as_str()))
        .chain(args_files.iter().map(|f| f.archive_path.as_str()))
        .chain(
            artifacts
                .plugin_wasms
                .iter()
                .map(|nb| nb.archive_path.as_str()),
        )
        .chain(
            artifacts
                .plugin_dirs
                .iter()
                .map(|d| d.archive_prefix.as_str()),
        )
        .chain(
            artifacts
                .plugin_files
                .iter()
                .map(|f| f.archive_path.as_str()),
        )
        .collect();
    validate_archive_paths(&paths)?;

    let file = File::create(output.as_std_path()).context(CreateOutputSnafu {
        path: output.to_path_buf(),
    })?;
    let gz = GzEncoder::new(BufWriter::new(file), Compression::default());
    let mut archive = ArchiveWriter::new(gz);

    for manifest in manifests {
        archive.bytes(&manifest.archive_path, manifest.yaml.as_bytes())?;
    }

    if let Some(app) = app_manifest {
        archive.bytes(APP_MANIFEST, app.yaml.as_bytes())?;
        for shot in &app.images {
            let data = fs::read(&shot.src_path).context(ReadImageSnafu {
                path: shot.src_path.clone(),
            })?;
            archive.bytes(&shot.archive_path, &data)?;
        }
    }

    for nb in &artifacts.wasms {
        archive.bytes(&nb.archive_path, &nb.bytes)?;
    }

    for entry in args_files {
        let data = fs::read(&entry.src_path).context(ReadArgsFileSnafu {
            path: entry.src_path.clone(),
        })?;
        archive.bytes(&entry.archive_path, &data)?;
    }

    for nb in &artifacts.plugin_wasms {
        archive.bytes(&nb.archive_path, &nb.bytes)?;
    }

    for d in &artifacts.plugin_dirs {
        archive.dir(&d.src_path, &d.archive_prefix)?;
    }

    for pf in &artifacts.plugin_files {
        let data = fs::read(&pf.src_path).context(ReadPluginFileSnafu {
            canister: pf.canister_name.clone(),
            file: pf.orig_file.clone(),
        })?;
        archive.bytes(&pf.archive_path, &data)?;
    }

    // Finalize the tar trailer, the gzip trailer, and the underlying BufWriter — any of these
    // may fail to write the last bytes to disk, and we want to surface that.
    let gz = archive.into_inner()?;
    let buf = gz.finish().context(FlushArchiveSnafu)?;
    buf.into_inner().map_err(|e| BundleError::FlushArchive {
        source: e.into_error(),
    })?;

    Ok(())
}

/// Up-front validation that the canister set can be bundled:
///  - no sync step is a script (we cannot replay an arbitrary shell command from the bundle)
///  - within each instance, all sanitized canister names are unique (otherwise archive paths
///    collide silently). Names only have to be distinct per instance, because each instance
///    writes its artifacts under its own archive directory.
fn validate_canisters(instances: &[Instance]) -> Result<(), BundleError> {
    for instance in instances {
        for (_, canister) in &instance.canisters {
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
        for (_, canister) in &instance.canisters {
            by_segment
                .entry(path_segment(local_name(&canister.name)))
                .or_default()
                .push(canister.name.clone());
        }
        for (sanitized, mut names) in by_segment {
            if names.len() > 1 {
                names.sort();
                return CanisterNameCollisionSnafu { sanitized, names }.fail();
            }
        }
    }

    Ok(())
}

/// Make every asset/plugin source path absolute and confirm it lives inside the
/// workspace root. Returns the canonical sync-directory paths for use in
/// output-overlap detection.
///
/// The workspace root — not the owning instance's directory — is the boundary: a
/// dependency's own sources are inside its directory and therefore inside the
/// root, while a source it reaches through a sibling (`../openemail/dist`) is
/// still copied into the archive and remains self-contained.
fn validate_source_paths(
    project_dir: &Path,
    canisters: &[(PathBuf, Canister)],
    canonical_project_dir: &Path,
) -> Result<Vec<PathBuf>, BundleError> {
    let mut canonical_sync_dirs = Vec::new();
    for (canister_path, canister) in canisters {
        for step in &canister.sync.steps {
            match step {
                SyncStep::Script(_) => {}
                SyncStep::Plugin(adapter) => {
                    if let Some(dirs) = &adapter.dirs {
                        for dir in dirs.entries() {
                            let src = canister_path.join(dir.path);
                            let resolved = resolve_within_project(
                                &src,
                                project_dir,
                                canonical_project_dir,
                                &canister.name,
                            )?;
                            canonical_sync_dirs.push(resolved);
                        }
                    }
                    if let Some(files) = &adapter.files {
                        for file in files.entries() {
                            let src = canister_path.join(file.path);
                            resolve_within_project(
                                &src,
                                project_dir,
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

/// Rejects a file backing an environment variable that lies outside the project.
/// The value is inlined into the bundled manifest, so without this a manifest
/// could name any file on the machine running `icp project bundle` and have its
/// contents written into an archive meant to be handed on.
///
/// Consolidation has already read these files — it resolves a canister's own
/// settings before `create_bundle` runs — so the paths come from the canister
/// model rather than the manifest, and the environment overrides `create_bundle`
/// rewrites itself are checked in [`inline_environments`].
fn validate_env_var_files(
    canisters: &[(PathBuf, Canister)],
    canonical_project_dir: &Path,
) -> Result<(), BundleError> {
    for (_, canister) in canisters {
        for (variable, file) in &canister.environment_variable_files {
            let canon = canonicalize(file)?;
            if !canon.starts_with(canonical_project_dir) {
                return EnvVarEscapesProjectSnafu {
                    canister: canister.name.clone(),
                    variable: variable.clone(),
                    path: file.clone(),
                    root: canonical_project_dir.to_path_buf(),
                }
                .fail();
            }
        }
    }
    Ok(())
}

/// Resolves a sync source path to its absolute location under the canonical
/// project directory, rejecting paths that escape the project — without touching
/// the filesystem.
///
/// Unlike [`canonicalize_within_project`], this performs no syscalls and does not
/// require the path to exist. Sync directories are frequently produced by a
/// canister's own build step (e.g. a frontend `dist` from `npm run build`), so
/// they do not exist yet when this validation runs, before the build. Validating
/// lexically keeps that feedback early instead of deferring it until after a
/// potentially slow build.
///
/// Every canister path is rooted at `project_dir`, so the path's project-relative
/// portion is recovered with `strip_prefix` and its `.`/`..` components resolved
/// textually; a `..` that rises above the root — or an absolute/sibling path that
/// `strip_prefix` rejects — is an escape. Symlinks are deliberately not resolved:
/// the archive step records them verbatim (`follow_symlinks(false)`), and a
/// not-yet-built directory cannot be a symlink anyway.
fn resolve_within_project(
    src: &Path,
    project_dir: &Path,
    canonical_project_dir: &Path,
    canister: &str,
) -> Result<PathBuf, BundleError> {
    let escapes = || {
        SourceEscapesProjectSnafu {
            canister: canister.to_owned(),
            path: src.to_path_buf(),
            root: canonical_project_dir.to_path_buf(),
        }
        .build()
    };

    let project_dir = project_dir.strip_prefix(".").unwrap_or(project_dir);
    let rel = src.strip_prefix(project_dir).map_err(|_| escapes())?;

    let mut components: Vec<&str> = Vec::new();
    for component in rel.components() {
        match component {
            Utf8Component::Normal(c) => components.push(c),
            Utf8Component::CurDir => {}
            // A `..` with nothing left to pop rises above the project root.
            Utf8Component::ParentDir => {
                components.pop().ok_or_else(escapes)?;
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => return Err(escapes()),
        }
    }

    let mut resolved = canonical_project_dir.to_path_buf();
    resolved.extend(components);
    Ok(resolved)
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

fn convert_args(args: &CanisterArgs) -> ManifestArgs {
    match args {
        CanisterArgs::Text { content, format } => ManifestArgs::Value {
            value: content.clone(),
            format: format.clone(),
        },
        CanisterArgs::Binary(bytes) => ManifestArgs::Value {
            value: hex::encode(bytes),
            format: ArgsFormat::Hex,
        },
    }
}
