use std::collections::HashSet;
use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use crate::fs;
use crate::prelude::*;

pub(crate) mod adapter;
pub(crate) mod canister;
pub(crate) mod dependency;
pub(crate) mod environment;
pub(crate) mod network;
pub(crate) mod project;
pub(crate) mod recipe;
pub(crate) mod serde_helpers;

pub use {
    adapter::plugin,
    adapter::prebuilt,
    canister::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, Instructions, ManifestInitArgs,
        SyncStep, SyncSteps,
    },
    dependency::DependencyManifest,
    environment::EnvironmentManifest,
    network::{ManagedMode, Mode, NetworkManifest},
    project::ProjectManifest,
};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "canister.yaml";

// A manifest item that can either be a path to another manifest file or the manifest itself.
//
// The valid path specifications are:
// - CanisterManifest: path or glob pattern to the directory containing "canister.yaml"
// - NetworkManifest: path to network manifest
// - EnvironmentManifest: path to environment manifest
#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
}

/// Items in path form serialize back to a bare path string, *not* to the contents of the
/// referenced file. Callers that need a self-contained YAML output (e.g. `icp project bundle`)
/// must convert any `Item::Path` to `Item::Manifest` themselves by loading the referenced
/// manifest first.
impl<T: Serialize> Serialize for Item<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Item::Path(p) => p.serialize(serializer),
            Item::Manifest(m) => m.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for Item<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor, value::MapAccessDeserializer};
        use std::fmt;

        struct ItemVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ItemVisitor<T> {
            type Value = Item<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string path or a manifest object")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Item::Path(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Item::Path(v))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                T::deserialize(MapAccessDeserializer::new(map)).map(Item::Manifest)
            }
        }

        deserializer.deserialize_any(ItemVisitor(PhantomData))
    }
}

#[derive(Debug, Snafu)]
pub enum ProjectRootLocateError {
    #[snafu(display("project manifest not found in {path}"))]
    NotFound { path: PathBuf },
}

/// Trait for locating the project root directory containing the project manifest file (`icp.yaml`).
pub trait ProjectRootLocate: Sync + Send {
    /// Locate the workspace root directory: the top-most project that
    /// transitively declares the project the command is standing in.
    fn locate(&self) -> Result<PathBuf, ProjectRootLocateError>;

    /// Locate the *member* directory the command is standing in: the nearest
    /// project manifest at or above cwd, without climbing to the workspace root.
    /// Equals [`locate`](Self::locate) at the root or in a standalone project.
    fn locate_member(&self) -> Result<PathBuf, ProjectRootLocateError>;
}

/// Implementation of [`ProjectRootLocate`].
pub struct ProjectRootLocateImpl {
    /// Current directory to begin search from in case dir is unspecified.
    cwd: PathBuf,

    /// Specific directory to be used as project root directly.
    dir: Option<PathBuf>,
}

impl ProjectRootLocateImpl {
    /// Creates a new instance of `ProjectRootLocateImpl`.
    ///
    /// - If `dir` is specified, it will be used as Project Root directly.
    /// - Otherwise, it will search upwards from `cwd` for the project manifest file (`icp.yaml`).
    pub fn new(cwd: PathBuf, dir: Option<PathBuf>) -> Self {
        Self { cwd, dir }
    }
}

/// The nearest directory at or above `start` that contains a project manifest.
fn nearest_manifest_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_owned();
    loop {
        if dir.join(PROJECT_MANIFEST).exists() {
            return Some(dir);
        }
        dir = dir.parent()?.to_owned();
    }
}

/// The nearest directory *strictly above* `dir` that contains a project manifest.
fn next_manifest_dir_above(dir: &Path) -> Option<PathBuf> {
    let mut cur = dir.parent()?.to_owned();
    loop {
        if cur.join(PROJECT_MANIFEST).exists() {
            return Some(cur);
        }
        cur = cur.parent()?.to_owned();
    }
}

/// Canonicalize a directory (resolving `..` and symlinks) into a UTF-8 path.
/// Returns `None` if the path does not exist or is not valid UTF-8; callers
/// treat that as "cannot establish identity", which is safe for resolution.
fn canonicalize_dir(dir: &Path) -> Option<PathBuf> {
    let canon = dunce::canonicalize(dir.as_std_path()).ok()?;
    PathBuf::try_from(canon).ok()
}

/// Read only the dependency `path:` entries from a manifest, ignoring every
/// other field. Deliberately lenient: any read/parse failure yields no
/// dependencies, so an unrelated or malformed ancestor manifest is treated as
/// declaring nothing (it will not be adopted as a workspace root).
fn read_dependency_paths(manifest_path: &Path) -> Vec<String> {
    #[derive(Deserialize)]
    struct DepProbe {
        path: String,
    }
    #[derive(Deserialize)]
    struct ManifestProbe {
        #[serde(default)]
        dependencies: Vec<DepProbe>,
    }

    let Ok(content) = fs::read(manifest_path) else {
        return Vec::new();
    };
    match serde_yaml::from_slice::<ManifestProbe>(&content) {
        Ok(p) => p.dependencies.into_iter().map(|d| d.path).collect(),
        Err(_) => Vec::new(),
    }
}

/// The set of canonical directories a manifest declares as dependencies,
/// transitively. Each `path:` is resolved relative to the manifest that
/// declares it, then canonicalized so identity is independent of how the path
/// is spelled (matches [`crate::project`] dependency de-duplication).
fn transitive_dep_dirs(manifest_dir: &Path) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    let Some(start) = canonicalize_dir(manifest_dir) else {
        return out;
    };
    let mut visited: HashSet<PathBuf> = HashSet::from([start.clone()]);
    let mut stack = vec![start];
    while let Some(dir) = stack.pop() {
        for rel in read_dependency_paths(&dir.join(PROJECT_MANIFEST)) {
            let Some(dep) = canonicalize_dir(&dir.join(&rel)) else {
                continue;
            };
            out.insert(dep.clone());
            if visited.insert(dep.clone()) {
                stack.push(dep);
            }
        }
    }
    out
}

impl ProjectRootLocate for ProjectRootLocateImpl {
    fn locate(&self) -> Result<PathBuf, ProjectRootLocateError> {
        // Start from the project the command is standing in. An explicit
        // override forces member == root (no climb) — see `locate_member`.
        let start = self.locate_member()?;
        if self.dir.is_some() {
            return Ok(start);
        }

        // Climb to the workspace root: adopt an ancestor only if its transitive
        // dependency closure declares `start`. Early-stop at the first ancestor
        // that does not — this never crosses a "gap" and never adopts an
        // unrelated ancestor (see DESIGN §16). With no declaring ancestor this
        // degenerates to returning `start`, i.e. today's behavior.
        let start_canonical = canonicalize_dir(&start).unwrap_or_else(|| start.clone());
        let mut root = start.clone();
        let mut cursor = start;
        while let Some(ancestor) = next_manifest_dir_above(&cursor) {
            if transitive_dep_dirs(&ancestor).contains(&start_canonical) {
                root = ancestor.clone();
                cursor = ancestor;
            } else {
                break;
            }
        }
        Ok(root)
    }

    fn locate_member(&self) -> Result<PathBuf, ProjectRootLocateError> {
        // An explicit override (`--project-root-override` / `ICP_PROJECT_ROOT`)
        // forces the project directory and skips the upward climb — the escape
        // hatch for "operate on exactly this project" (e.g. deploy a vendored
        // member as a standalone project). Member and root are then identical.
        if let Some(dir) = &self.dir {
            if !dir.join(PROJECT_MANIFEST).exists() {
                return NotFoundSnafu {
                    path: dir.to_owned(),
                }
                .fail();
            }

            return Ok(dir.to_owned());
        }

        // The project the command is standing in: nearest manifest at/above cwd.
        nearest_manifest_dir(&self.cwd).ok_or_else(|| {
            NotFoundSnafu {
                path: self.cwd.to_owned(),
            }
            .build()
        })
    }
}

#[derive(Debug, Snafu)]
pub enum LoadManifestFromPathError {
    #[snafu(display("failed to read manifest from path"))]
    Read { source: fs::IoError },

    #[snafu(display("failed to parse manifest at '{path}'"))]
    Parse {
        source: serde_yaml::Error,
        path: PathBuf,
    },
}

/// Loads a manifest of type `T` from the specified file path.
pub async fn load_manifest_from_path<T>(path: &Path) -> Result<T, LoadManifestFromPathError>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read(path).context(ReadSnafu)?;
    let m = serde_yaml::from_slice::<T>(&content).context(ParseSnafu {
        path: path.to_path_buf(),
    })?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino_tempfile::Utf8TempDir;

    fn write_manifest(dir: &Path) {
        std::fs::write(dir.join(PROJECT_MANIFEST), "").unwrap();
    }

    /// Create `dir` (and parents) and write an `icp.yaml` declaring the given
    /// `(alias, path)` dependencies.
    fn write_project(dir: &Path, deps: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut body = String::new();
        if !deps.is_empty() {
            body.push_str("dependencies:\n");
            for (name, path) in deps {
                body.push_str(&format!("  - name: {name}\n    path: {path}\n"));
            }
        }
        std::fs::write(dir.join(PROJECT_MANIFEST), body).unwrap();
    }

    // A lone project (no declaring ancestor) is its own root.
    #[test]
    fn locate_standalone_member_is_its_own_root() {
        let tmp = Utf8TempDir::new().unwrap();
        let member = tmp.path().join("openemail");
        write_project(&member, &[]);

        let locator = ProjectRootLocateImpl::new(member.clone(), None);
        assert_eq!(locator.locate().unwrap(), member);
    }

    // Running inside a member climbs to the parent that declares it.
    #[test]
    fn locate_climbs_to_declaring_parent() {
        let tmp = Utf8TempDir::new().unwrap();
        let openhr = tmp.path().join("openhr");
        write_project(&openhr, &[("openemail", "./openemail")]);
        let openemail = openhr.join("openemail");
        write_project(&openemail, &[]);

        let locator = ProjectRootLocateImpl::new(openemail, None);
        assert_eq!(locator.locate().unwrap(), openhr);
    }

    // A transitive chain climbs all the way to the top-most declaring project,
    // from any member in the chain.
    #[test]
    fn locate_climbs_transitive_chain_to_top() {
        let tmp = Utf8TempDir::new().unwrap();
        let app = tmp.path().join("app");
        write_project(&app, &[("openhr", "./openhr")]);
        let openhr = app.join("openhr");
        write_project(&openhr, &[("openemail", "./openemail")]);
        let openemail = openhr.join("openemail");
        write_project(&openemail, &[]);

        assert_eq!(
            ProjectRootLocateImpl::new(openemail, None)
                .locate()
                .unwrap(),
            app
        );
        assert_eq!(
            ProjectRootLocateImpl::new(openhr, None).locate().unwrap(),
            app
        );
    }

    // Diamond: the shared member is declared via siblings, not directly by the
    // top project, and sits at a hoisted location. Transitive containment still
    // resolves the top project as root.
    #[test]
    fn locate_resolves_diamond_via_transitive_closure() {
        let tmp = Utf8TempDir::new().unwrap();
        let app = tmp.path().join("app");
        write_project(
            &app,
            &[
                ("service_a", "./umbrella/service-a"),
                ("service_b", "./umbrella/service-b"),
            ],
        );
        write_project(
            &app.join("umbrella/service-a"),
            &[("openemail", "../openemail")],
        );
        write_project(
            &app.join("umbrella/service-b"),
            &[("openemail", "../openemail")],
        );
        let openemail = app.join("umbrella/openemail");
        write_project(&openemail, &[]);

        // `umbrella/` has no manifest, so the nearest ancestor above openemail is
        // `app`, which declares openemail only transitively (app -> service-a ->
        // ../openemail).
        assert_eq!(
            ProjectRootLocateImpl::new(openemail, None)
                .locate()
                .unwrap(),
            app
        );
    }

    // An ancestor that does not declare the project is not adopted as root.
    #[test]
    fn locate_rejects_unrelated_ancestor() {
        let tmp = Utf8TempDir::new().unwrap();
        let outer = tmp.path().join("outer");
        write_project(&outer, &[]); // declares nothing
        let app = outer.join("app");
        write_project(&app, &[]);

        let locator = ProjectRootLocateImpl::new(app.clone(), None);
        assert_eq!(locator.locate().unwrap(), app);
    }

    // Gap: a declaring project sits above a non-declaring manifest. Early-stop
    // stops at the contiguous declaring chain and does not cross the gap.
    #[test]
    fn locate_early_stops_at_gap() {
        let tmp = Utf8TempDir::new().unwrap();
        let outer = tmp.path().join("outer");
        // outer declares openhr through the gap directory.
        write_project(&outer, &[("openhr", "./legacy/openhr")]);
        let legacy = outer.join("legacy");
        write_project(&legacy, &[]); // the gap: declares nothing
        let openhr = legacy.join("openhr");
        write_project(&openhr, &[("openemail", "./openemail")]);
        let openemail = openhr.join("openemail");
        write_project(&openemail, &[]);

        // Climb stops at openhr because `legacy` (the next ancestor) does not
        // declare openemail, even though `outer` above it does.
        let locator = ProjectRootLocateImpl::new(openemail, None);
        assert_eq!(locator.locate().unwrap(), openhr);
    }

    // An explicit override forces that directory as root, with no upward climb.
    #[test]
    fn locate_override_forces_root_without_climbing() {
        let tmp = Utf8TempDir::new().unwrap();
        let openhr = tmp.path().join("openhr");
        write_project(&openhr, &[("openemail", "./openemail")]);
        let openemail = openhr.join("openemail");
        write_project(&openemail, &[]);

        // cwd is openemail but override pins openemail itself as the root.
        let locator = ProjectRootLocateImpl::new(openemail.clone(), Some(openemail.clone()));
        assert_eq!(locator.locate().unwrap(), openemail);
    }

    #[test]
    fn locate_returns_cwd_when_manifest_present() {
        let tmp = Utf8TempDir::new().unwrap();
        write_manifest(tmp.path());

        let locator = ProjectRootLocateImpl::new(tmp.path().to_path_buf(), None);
        assert_eq!(locator.locate().unwrap(), tmp.path());
    }

    #[test]
    fn locate_walks_up_to_manifest() {
        let tmp = Utf8TempDir::new().unwrap();
        write_manifest(tmp.path());

        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();

        let locator = ProjectRootLocateImpl::new(nested, None);
        assert_eq!(locator.locate().unwrap(), tmp.path());
    }

    #[test]
    fn locate_returns_not_found_when_no_manifest_anywhere() {
        let tmp = Utf8TempDir::new().unwrap();
        let nested = tmp.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();

        // Host filesystem contains no icp.yaml above the tempdir (assumed in CI).
        let locator = ProjectRootLocateImpl::new(nested, None);
        assert!(matches!(
            locator.locate(),
            Err(ProjectRootLocateError::NotFound { .. })
        ));
    }

    // When cwd is a symlinked directory, locate() walks up via the symlink's
    // lexical parents
    #[cfg(unix)]
    #[test]
    fn locate_walks_up_through_symlink() {
        // target/ has no manifest anywhere above it within the test's scope.
        let target = Utf8TempDir::new().unwrap();

        // project/ contains the manifest; `project/link` is a symlink to target/.
        let project = Utf8TempDir::new().unwrap();
        write_manifest(project.path());
        let link = project.path().join("link");
        std::os::unix::fs::symlink(target.path().as_std_path(), link.as_std_path()).unwrap();

        // cwd is the symlink path; its lexical parent is `project`,
        // which contains the manifest.
        let locator = ProjectRootLocateImpl::new(link, None);
        assert_eq!(locator.locate().unwrap(), project.path());
    }
}
