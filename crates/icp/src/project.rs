//! Host-side project facade.
//!
//! Consolidation of manifests into a [`Project`] lives in
//! `icp_deploy_canister::project` (over an injected `FileAccess`) and is
//! re-exported here. Walking a workspace's dependency edges on disk and
//! member-scoping both resolve real filesystem paths against the current working
//! directory, so they stay here.

use std::collections::HashSet;

use snafu::prelude::*;

pub use icp_deploy_canister::project::{
    ConsolidateManifestError, EnvironmentError, LoadProjectError, VerifySandboxError,
    consolidate_manifest, load_project, relative_prefix, verify_sandbox,
};

use crate::{
    Environment,
    manifest::{
        LoadManifestFromPathError, PROJECT_MANIFEST, ProjectManifest, load_manifest_from_path,
    },
    prelude::*,
};

/// Canonicalize into a UTF-8 path, or `None` if it does not exist / is not UTF-8.
fn canonicalize_or(dir: &Path) -> Option<PathBuf> {
    let canon = dunce::canonicalize(dir.as_std_path()).ok()?;
    PathBuf::try_from(canon).ok()
}

/// One project in a workspace: the root project, or a dependency instance
/// reachable from it. Returned by [`workspace_instances`].
#[derive(Debug)]
pub struct WorkspaceInstance {
    /// Store-key prefix for this instance's canisters — its canonical directory
    /// relative to the canonical workspace root, forward-slash separated. Empty
    /// for the root project, whose canisters are keyed by their bare names.
    pub prefix: String,

    /// The instance's directory as reached from the workspace root, and the base
    /// for resolving the relative paths inside [`Self::manifest`]. Not
    /// canonicalized, so it keeps the spelling the declaring manifest used.
    pub dir: PathBuf,

    /// The instance's manifest, exactly as written on disk.
    pub manifest: ProjectManifest,

    /// The store-key prefix of the instance each of this instance's
    /// `dependencies` entries resolves to, index-aligned with
    /// `manifest.dependencies`.
    ///
    /// A declared `path:` need not agree with the instance's location relative to
    /// the workspace root — it can be absolute, or traverse a symlink — so a
    /// caller reproducing the workspace elsewhere cannot derive the target from
    /// the spelling alone.
    pub dependency_prefixes: Vec<String>,

    /// The `(alias, path)` of the first dependency edge that reached this
    /// instance, for error messages. `None` for the root project.
    pub declared_as: Option<(String, String)>,
}

#[derive(Debug, Snafu)]
pub enum WorkspaceInstancesError {
    #[snafu(display("failed to load the project manifest at '{path}'"))]
    LoadWorkspaceRoot {
        path: PathBuf,
        source: LoadManifestFromPathError,
    },

    #[snafu(display("could not find a project manifest for dependency '{alias}' at: '{path}'"))]
    InstanceNotFound { alias: String, path: PathBuf },

    #[snafu(display("failed to canonicalize path for dependency '{alias}' at: '{path}'"))]
    InstanceCanonicalize { alias: String, path: PathBuf },

    #[snafu(display("failed to load project manifest for dependency '{alias}' at '{path}'"))]
    LoadInstance {
        alias: String,
        path: PathBuf,
        source: LoadManifestFromPathError,
    },
}

/// One dependency declaration resolved to the instance it points at.
struct ResolvedEdge {
    /// The instance's directory as the declaring manifest reaches it.
    dir: PathBuf,
    canonical: PathBuf,
    /// Store-key prefix of the instance this edge resolves to.
    prefix: String,
    alias: String,
    path: String,
}

/// Resolve one project's dependency declarations, in declaration order.
fn resolve_edges(
    dir: &Path,
    manifest: &ProjectManifest,
    app_root_canonical: &Path,
) -> Result<Vec<ResolvedEdge>, WorkspaceInstancesError> {
    let mut out = Vec::with_capacity(manifest.dependencies.len());
    for dep in &manifest.dependencies {
        let dep_root = dir.join(&dep.path);
        if !dep_root.join(PROJECT_MANIFEST).is_file() {
            return InstanceNotFoundSnafu {
                alias: dep.name.clone(),
                path: dep_root,
            }
            .fail();
        }
        let canonical = canonicalize_or(&dep_root).context(InstanceCanonicalizeSnafu {
            alias: &dep.name,
            path: &dep_root,
        })?;
        out.push(ResolvedEdge {
            prefix: relative_prefix(app_root_canonical, &canonical),
            dir: dep_root,
            canonical,
            alias: dep.name.clone(),
            path: dep.path.clone(),
        });
    }
    Ok(out)
}

/// Walk the workspace rooted at `pdir`, returning the root project followed by
/// every dependency instance reachable from it, in depth-first declaration
/// order.
///
/// This retraces the same edges `import_dependency` follows — de-duplicating
/// instances by canonical directory, so a diamond yields one entry whose
/// `prefix` matches the store-key prefix its canisters were assigned — but keeps
/// each instance's *raw* manifest instead of folding its canisters into the
/// workspace. Callers that need to reproduce the workspace structure (notably
/// `icp project bundle`) need the dependency edges and per-instance environments,
/// which consolidation deliberately flattens away.
///
/// Cycles are rejected by [`consolidate_manifest`], which every caller runs
/// first; the visited set here only keeps the walk finite.
pub async fn workspace_instances(
    pdir: &Path,
) -> Result<Vec<WorkspaceInstance>, WorkspaceInstancesError> {
    let root_manifest_path = pdir.join(PROJECT_MANIFEST);
    let root_manifest: ProjectManifest = load_manifest_from_path(&root_manifest_path)
        .await
        .context(LoadWorkspaceRootSnafu {
            path: &root_manifest_path,
        })?;

    // Same fallback as `consolidate_manifest`, so prefixes agree with the store
    // keys even when the root directory cannot be canonicalized.
    let app_root_canonical = canonicalize_or(pdir).unwrap_or_else(|| pdir.to_owned());

    let root_edges = resolve_edges(pdir, &root_manifest, &app_root_canonical)?;

    // Depth-first, declaration order: push each instance's dependencies reversed
    // so the top of the stack is always the next edge in manifest order.
    let mut visited: HashSet<PathBuf> = HashSet::from([app_root_canonical.clone()]);
    let mut out = vec![WorkspaceInstance {
        prefix: String::new(),
        dir: pdir.to_owned(),
        manifest: root_manifest,
        dependency_prefixes: root_edges.iter().map(|e| e.prefix.clone()).collect(),
        declared_as: None,
    }];
    let mut stack: Vec<ResolvedEdge> = root_edges.into_iter().rev().collect();

    while let Some(edge) = stack.pop() {
        if !visited.insert(edge.canonical.clone()) {
            continue;
        }

        let manifest_path = edge.dir.join(PROJECT_MANIFEST);
        let manifest: ProjectManifest =
            load_manifest_from_path(&manifest_path)
                .await
                .context(LoadInstanceSnafu {
                    alias: &edge.alias,
                    path: &manifest_path,
                })?;

        let edges = resolve_edges(&edge.dir, &manifest, &app_root_canonical)?;
        let dependency_prefixes = edges.iter().map(|e| e.prefix.clone()).collect();
        stack.extend(edges.into_iter().rev());

        out.push(WorkspaceInstance {
            prefix: edge.prefix,
            dir: edge.dir,
            manifest,
            dependency_prefixes,
            declared_as: Some((edge.alias, edge.path)),
        });
    }

    Ok(out)
}

/// The default set of target canisters when the user names none, honoring
/// member-scoping.
///
/// When the command is run inside a vendored member — `member_dir` is a distinct
/// directory below the workspace `root_dir` — only that member's canisters are
/// targeted: those whose directory lies within `member_dir` (the member's own
/// canisters plus any dependencies nested under it). Dependencies hoisted
/// outside the member are assumed already deployed and keep their ids in the
/// shared root store, so cross-member wiring stays valid.
///
/// Returns `None` meaning "no scoping — target the whole environment": at the
/// workspace root or a standalone project (`member_dir` resolves to `root_dir`),
/// when `member_dir` is unknown, or when paths cannot be resolved.
pub fn member_scoped_canisters(
    root_dir: &Path,
    member_dir: Option<&Path>,
    env: &Environment,
) -> Option<Vec<String>> {
    let member = member_dir?;
    let root_c = canonicalize_or(root_dir)?;
    let member_c = canonicalize_or(member)?;
    if root_c == member_c {
        return None;
    }

    let names = env
        .canisters
        .iter()
        .filter(|(_, (dir, _))| {
            canonicalize_or(dir).is_some_and(|c| c == member_c || c.starts_with(&member_c))
        })
        .map(|(name, _)| name.clone())
        .collect();
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canister::recipe::{RemoteResourceResolve, ResolveError};
    use crate::host_files::HostFileAccess;
    use crate::manifest::adapter::prebuilt::SourceField;
    use crate::manifest::recipe::Recipe;
    use crate::manifest::{PROJECT_MANIFEST, ProjectManifest, load_manifest_from_path};
    use crate::prelude::LOCAL;
    use camino_tempfile::Utf8TempDir;
    use icp_deploy_canister::sync_exec::StepProgress;

    /// Recipes and plugins are never used in this test; every canister is pre-built.
    struct PanicResolver;

    #[async_trait::async_trait]
    impl RemoteResourceResolve for PanicResolver {
        async fn resolve_recipe(&self, _recipe: &Recipe) -> Result<String, ResolveError> {
            panic!("recipe resolver should not be called in this test");
        }

        async fn resolve_wasm(
            &self,
            _source: &SourceField,
            _base_dir: &Path,
            _sha256: Option<&str>,
            _progress: Option<&dyn StepProgress>,
        ) -> Result<PathBuf, ResolveError> {
            panic!("wasm resolver should not be called in this test");
        }
    }

    fn write(dir: &Path, rel: &str, contents: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    fn manifest(canisters: &[&str], deps: &str) -> String {
        let mut s = String::new();
        if canisters.is_empty() {
            s.push_str("canisters: []\n");
        } else {
            s.push_str("canisters:\n");
            for c in canisters {
                s.push_str(&format!(
                    "  - name: {c}\n    build:\n      steps:\n        - type: pre-built\n          path: {c}.wasm\n"
                ));
            }
        }
        s.push_str(deps);
        s
    }

    #[tokio::test]
    async fn member_scope_targets_only_the_members_canisters() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend", "frontend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: openemail\n    path: ./openemail\n",
            ),
        );

        let m: ProjectManifest = load_manifest_from_path(&tmp.path().join(PROJECT_MANIFEST))
            .await
            .unwrap();
        let p = consolidate_manifest(&HostFileAccess, tmp.path(), &PanicResolver, &m)
            .await
            .unwrap();
        let env = p.environments.get(LOCAL).expect("local environment");

        // At the workspace root (member == root): no scoping.
        assert_eq!(member_scoped_canisters(&p.dir, Some(&p.dir), env), None);

        // Unknown member dir: no scoping.
        assert_eq!(member_scoped_canisters(&p.dir, None, env), None);

        // Inside the member: only the member's own canisters, not the app's.
        let member = tmp.path().join("openemail");
        let mut scoped = member_scoped_canisters(&p.dir, Some(&member), env)
            .expect("should scope when inside a member");
        scoped.sort();
        assert_eq!(
            scoped,
            vec![
                "openemail:backend".to_string(),
                "openemail:frontend".to_string(),
            ]
        );
    }
}
