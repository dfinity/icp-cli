//! Host-side project facade.
//!
//! Consolidation of manifests into a [`Project`] lives in
//! `icp_deploy_canister::project` (over an injected `FileAccess`) and is
//! re-exported here. Member-scoping resolves real filesystem paths, so it stays
//! host-side.

pub use icp_deploy_canister::project::{
    ConsolidateManifestError, EnvironmentError, LoadProjectError, VerifySandboxError,
    consolidate_manifest, load_project, verify_sandbox,
};

use crate::{Environment, prelude::*};

/// Canonicalize into a UTF-8 path, or `None` if it does not exist / is not UTF-8.
fn canonicalize_or(dir: &Path) -> Option<PathBuf> {
    let canon = dunce::canonicalize(dir.as_std_path()).ok()?;
    PathBuf::try_from(canon).ok()
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
    use crate::canister::recipe::{RecipeContext, RemoteResourceResolve, ResolveError};
    use crate::manifest::adapter::prebuilt::SourceField;
    use crate::manifest::canister::{BuildSteps, SyncSteps};
    use crate::manifest::recipe::Recipe;
    use crate::manifest::{PROJECT_MANIFEST, ProjectManifest, load_manifest_from_path};
    use crate::prelude::LOCAL;
    use camino_tempfile::Utf8TempDir;
    use tokio::sync::mpsc::Sender;

    /// Recipes and plugins are never used in this test; every canister is pre-built.
    struct PanicResolver;

    #[async_trait::async_trait]
    impl RemoteResourceResolve for PanicResolver {
        async fn resolve_recipe(
            &self,
            _recipe: &Recipe,
            _context: &RecipeContext,
        ) -> Result<(BuildSteps, SyncSteps), ResolveError> {
            panic!("recipe resolver should not be called in this test");
        }

        async fn resolve_wasm(
            &self,
            _source: &SourceField,
            _base_dir: &Path,
            _sha256: Option<&str>,
            _stdio: Option<Sender<String>>,
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
        let p = consolidate_manifest(tmp.path(), &PanicResolver, &m)
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
