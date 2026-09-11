//! Reading and writing the files a project is made of.
//!
//! Consolidating a manifest means reading the manifests it points at, the files
//! its arguments and environment variables come from, and the directories its
//! globs expand over. Building means reading back the module a build step
//! produced. None of that is necessarily a filesystem: the same project could
//! be described by blobs in a canister's stable memory. So it is asked for
//! through [`FileSystem`] rather than done here.
//!
//! [`fs`](crate::fs) is the host implementation's own vocabulary — thin
//! wrappers over `std::fs` whose errors carry the path. Project code should
//! not reach for it.

use async_trait::async_trait;
use camino::Utf8Component;
use snafu::{ResultExt, Snafu};

use crate::prelude::*;

/// A file operation failed.
///
/// What backs the files is the implementation's business — a real filesystem, a
/// bundle being unpacked, stable memory — so the cause is carried whole and
/// displayed as itself. Implementations name the path in their own error, which
/// is what a reader needs to see.
#[derive(Debug, Snafu)]
#[snafu(transparent)]
pub struct FsError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl FsError {
    /// Wraps an implementation's own error for the trait boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

/// Where a project's files come from.
///
/// The predicates (`exists`, `is_file`, `is_dir`) answer `false` on any error,
/// matching the `camino` inherent methods they replace: a caller asking whether
/// something is there has nothing useful to do with the difference between "no"
/// and "could not tell". [`canonicalize`](FileSystem::canonicalize) returns
/// `None` for the same reason, and callers treat that as "cannot establish
/// identity", which is the safe answer when the question is whether two paths
/// are the same file.
#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn read(&self, path: &Path) -> Result<Vec<u8>, FsError>;

    async fn read_to_string(&self, path: &Path) -> Result<String, FsError>;

    async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError>;

    async fn create_dir_all(&self, path: &Path) -> Result<(), FsError>;

    async fn copy(&self, from: &Path, to: &Path) -> Result<(), FsError>;

    async fn exists(&self, path: &Path) -> bool;

    async fn is_file(&self, path: &Path) -> bool;

    async fn is_dir(&self, path: &Path) -> bool;

    /// Non-recursive listing. Entries come back as `path` joined with each
    /// entry's name, so they are usable as-is.
    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FsError>;

    /// Resolve `..` and symlinks. `None` when the path does not resolve.
    async fn canonicalize(&self, path: &Path) -> Option<PathBuf>;

    /// Somewhere to put files that only one caller needs and nothing keeps.
    ///
    /// A build step writes its module to a path handed to it and the operation
    /// reads it back, so the two have to meet somewhere — and it is this
    /// implementation, not the operation, that knows where a path it can read
    /// is allowed to come from.
    async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError>;
}

/// A directory that exists for as long as this is held, and is removed with it.
pub trait Scratch: Send + Sync {
    fn path(&self) -> &Path;
}

#[cfg(feature = "host")]
/// The [`FileSystem`] backed by this machine's filesystem.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostFileSystem;

#[cfg(feature = "host")]
#[async_trait]
impl FileSystem for HostFileSystem {
    async fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        crate::fs::read(path).map_err(FsError::new)
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, FsError> {
        crate::fs::read_to_string(path).map_err(FsError::new)
    }

    async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
        crate::fs::write(path, contents).map_err(FsError::new)
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), FsError> {
        crate::fs::create_dir_all(path).map_err(FsError::new)
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        crate::fs::copy(from, to).map_err(FsError::new)
    }

    async fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    async fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    async fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FsError> {
        crate::fs::read_dir(path).map_err(FsError::new)
    }

    async fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        PathBuf::from_path_buf(dunce::canonicalize(path).ok()?).ok()
    }

    async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
        let dir = camino_tempfile::tempdir().map_err(FsError::new)?;
        Ok(Box::new(HostScratch(dir)))
    }
}

/// A temporary directory of this machine's, which `tempfile` removes when the
/// [`Utf8TempDir`](camino_tempfile::Utf8TempDir) drops.
#[cfg(feature = "host")]
struct HostScratch(camino_tempfile::Utf8TempDir);

#[cfg(feature = "host")]
impl Scratch for HostScratch {
    fn path(&self) -> &Path {
        self.0.path()
    }
}

#[cfg(any(test, feature = "test-util"))]
/// A [`FileSystem`] for tests on paths that never reach a file.
pub struct UnimplementedMockFileSystem;

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl FileSystem for UnimplementedMockFileSystem {
    async fn read(&self, _path: &Path) -> Result<Vec<u8>, FsError> {
        unimplemented!("UnimplementedMockFileSystem::read")
    }

    async fn read_to_string(&self, _path: &Path) -> Result<String, FsError> {
        unimplemented!("UnimplementedMockFileSystem::read_to_string")
    }

    async fn write(&self, _path: &Path, _contents: &[u8]) -> Result<(), FsError> {
        unimplemented!("UnimplementedMockFileSystem::write")
    }

    async fn create_dir_all(&self, _path: &Path) -> Result<(), FsError> {
        unimplemented!("UnimplementedMockFileSystem::create_dir_all")
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> Result<(), FsError> {
        unimplemented!("UnimplementedMockFileSystem::copy")
    }

    async fn exists(&self, _path: &Path) -> bool {
        unimplemented!("UnimplementedMockFileSystem::exists")
    }

    async fn is_file(&self, _path: &Path) -> bool {
        unimplemented!("UnimplementedMockFileSystem::is_file")
    }

    async fn is_dir(&self, _path: &Path) -> bool {
        unimplemented!("UnimplementedMockFileSystem::is_dir")
    }

    async fn read_dir(&self, _path: &Path) -> Result<Vec<PathBuf>, FsError> {
        unimplemented!("UnimplementedMockFileSystem::read_dir")
    }

    async fn canonicalize(&self, _path: &Path) -> Option<PathBuf> {
        unimplemented!("UnimplementedMockFileSystem::canonicalize")
    }

    async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
        unimplemented!("UnimplementedMockFileSystem::scratch_dir")
    }
}

/// A glob pattern could not be understood.
#[derive(Debug, Snafu)]
pub enum GlobError {
    #[snafu(display("'{pattern}' is not a valid glob pattern"))]
    Pattern {
        source: glob::PatternError,
        pattern: String,
    },

    #[snafu(display("failed to list '{path}' while expanding glob '{pattern}'"))]
    List {
        source: FsError,
        path: PathBuf,
        pattern: String,
    },
}

/// Expand `pattern` against `files`, relative to `base`.
///
/// The `glob` crate walks the real filesystem itself, so it is no use where the
/// files are not one. Matching happens a component at a time instead, with
/// `**` standing for any number of directories — the same shape `glob`
/// supports, and the same one the manifest reference documents.
///
/// A component that cannot match anything is instead resolved the way joining
/// it onto `base` would resolve it: `..` climbs, and an absolute pattern starts
/// from its own root with `base` dropped. So a pattern with no metacharacters
/// at all names the same path here as `base.join(pattern)` does.
///
/// Only paths that exist are returned, and each directory's entries are listed
/// in sorted order, so the result is the same on every run.
pub async fn expand_glob(
    files: &dyn FileSystem,
    base: &Path,
    pattern: &str,
) -> Result<Vec<PathBuf>, GlobError> {
    let mut frontier = vec![base.to_path_buf()];

    for component in Path::new(pattern).components() {
        match component {
            // A root — or, on Windows, a drive prefix — is what makes a pattern
            // absolute. Pushing it is what leaves `base` behind, by the same
            // rule that joining an absolute path onto another discards the
            // other.
            Utf8Component::Prefix(_) | Utf8Component::RootDir => {
                for dir in &mut frontier {
                    dir.push(component.as_str());
                }
            }

            Utf8Component::CurDir => {}

            // No listing ever turns up an entry named `..`, so this names a
            // directory rather than matching one. It still has to be one, or
            // the pattern describes no path from here.
            Utf8Component::ParentDir => {
                let mut next = Vec::new();
                for dir in &frontier {
                    let parent = dir.join("..");
                    if files.is_dir(&parent).await {
                        next.push(parent);
                    }
                }
                frontier = next;
            }

            // `**` matches zero or more directories, so every reachable
            // directory — including the ones already in hand — carries forward.
            Utf8Component::Normal("**") => {
                let mut reached = frontier.clone();
                let mut stack = frontier;
                while let Some(dir) = stack.pop() {
                    for entry in list(files, &dir, pattern).await? {
                        if files.is_dir(&entry).await {
                            reached.push(entry.clone());
                            stack.push(entry);
                        }
                    }
                }
                reached.sort();
                reached.dedup();
                frontier = reached;
            }

            Utf8Component::Normal(component) => {
                let matcher = glob::Pattern::new(component).context(PatternSnafu {
                    pattern: pattern.to_owned(),
                })?;

                let mut next = Vec::new();
                for dir in &frontier {
                    for entry in list(files, dir, pattern).await? {
                        if entry.file_name().is_some_and(|name| matcher.matches(name)) {
                            next.push(entry);
                        }
                    }
                }
                next.sort();
                next.dedup();
                frontier = next;
            }
        }
    }

    Ok(frontier)
}

/// Entries of `dir`, or none when it cannot be listed because it is not a
/// directory. A glob reaching past a plain file matches nothing rather than
/// failing.
async fn list(
    files: &dyn FileSystem,
    dir: &Path,
    pattern: &str,
) -> Result<Vec<PathBuf>, GlobError> {
    if !files.is_dir(dir).await {
        return Ok(Vec::new());
    }
    files.read_dir(dir).await.context(ListSnafu {
        path: dir.to_path_buf(),
        pattern: pattern.to_owned(),
    })
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;

    /// Builds a tree of empty files, creating parents as needed, and returns its
    /// root.
    fn tree(paths: &[&str]) -> camino_tempfile::Utf8TempDir {
        let dir = camino_tempfile::Utf8TempDir::new().expect("temp dir");
        for p in paths {
            let full = dir.path().join(p);
            std::fs::create_dir_all(full.parent().expect("has a parent")).expect("mkdir");
            std::fs::write(&full, b"").expect("write");
        }
        dir
    }

    async fn expand(root: &Path, pattern: &str) -> Vec<String> {
        expand_from(root, root, pattern).await
    }

    /// As [`expand`], but expanding from a `base` the pattern may leave: results
    /// are still spelled relative to `root`.
    async fn expand_from(base: &Path, root: &Path, pattern: &str) -> Vec<String> {
        expand_glob(&HostFileSystem, base, pattern)
            .await
            .expect("expand")
            .into_iter()
            .map(|p| {
                p.strip_prefix(root)
                    .expect("under root")
                    .as_str()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[tokio::test]
    async fn a_single_star_matches_within_one_directory_only() {
        let d = tree(&["canisters/a/canister.yaml", "canisters/b/canister.yaml"]);
        assert_eq!(
            expand(d.path(), "canisters/*").await,
            ["canisters/a", "canisters/b"]
        );
    }

    /// A literal component is still matched by listing its parent, so it yields
    /// the one path it names — and nothing when that path is not there.
    #[tokio::test]
    async fn literal_components_name_one_path() {
        let d = tree(&["canisters/a/canister.yaml"]);
        assert_eq!(
            expand(d.path(), "canisters/a/canister.yaml").await,
            ["canisters/a/canister.yaml"]
        );
        assert!(
            expand(d.path(), "canisters/a/nothing.yaml")
                .await
                .is_empty()
        );
    }

    /// `**` stands for zero or more directories, so a pattern that uses it also
    /// matches at the depth where it stands for none.
    #[tokio::test]
    async fn a_double_star_matches_at_every_depth_including_zero() {
        let d = tree(&[
            "services/one.yaml",
            "services/a/two.yaml",
            "services/a/b/three.yaml",
            "elsewhere/four.yaml",
        ]);
        assert_eq!(
            expand(d.path(), "services/**/*.yaml").await,
            [
                "services/a/b/three.yaml",
                "services/a/two.yaml",
                "services/one.yaml",
            ]
        );
    }

    #[tokio::test]
    async fn character_classes_and_question_marks_work() {
        let d = tree(&["c/a1.yaml", "c/b2.yaml", "c/cc.yaml"]);
        assert_eq!(
            expand(d.path(), "c/?[0-9].yaml").await,
            ["c/a1.yaml", "c/b2.yaml"]
        );
    }

    /// Reaching through a plain file matches nothing, rather than failing: the
    /// pattern simply describes no path here.
    #[tokio::test]
    async fn descending_through_a_file_matches_nothing() {
        let d = tree(&["notadir"]);
        assert!(expand(d.path(), "notadir/*").await.is_empty());
    }

    #[tokio::test]
    async fn a_pattern_matching_nothing_yields_nothing() {
        let d = tree(&["canisters/a/canister.yaml"]);
        assert!(expand(d.path(), "services/*").await.is_empty());
    }

    #[tokio::test]
    async fn results_are_sorted_so_a_run_is_reproducible() {
        let d = tree(&["c/z/x", "c/a/x", "c/m/x"]);
        assert_eq!(expand(d.path(), "c/*").await, ["c/a", "c/m", "c/z"]);
    }

    /// A dependency next to the project, rather than under it, is named by
    /// climbing out of it — the shape a workspace of sibling projects uses.
    #[tokio::test]
    async fn a_parent_component_climbs_out_of_the_base() {
        let d = tree(&[
            "proj/icp.yaml",
            "shared/a/canister.yaml",
            "shared/b/canister.yaml",
        ]);
        assert_eq!(
            expand_from(&d.path().join("proj"), d.path(), "../shared/*").await,
            ["proj/../shared/a", "proj/../shared/b"]
        );
    }

    /// `..` has to name a directory that is there, like any other component.
    #[tokio::test]
    async fn climbing_to_nowhere_matches_nothing() {
        let d = tree(&["proj/icp.yaml"]);
        assert!(
            expand_from(&d.path().join("proj/nonexistent"), d.path(), "../*")
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn an_absolute_pattern_leaves_the_base_behind() {
        let d = tree(&["shared/a/canister.yaml", "shared/b/canister.yaml"]);
        let elsewhere = tree(&["unrelated/canister.yaml"]);
        let pattern = format!("{}/*", d.path().join("shared"));
        assert_eq!(
            expand_from(elsewhere.path(), d.path(), &pattern).await,
            ["shared/a", "shared/b"]
        );
    }

    #[tokio::test]
    async fn a_malformed_pattern_is_reported_as_such() {
        let d = tree(&["c/a"]);
        let err = expand_glob(&HostFileSystem, d.path(), "c/[a-")
            .await
            .expect_err("unterminated class");
        assert!(matches!(err, GlobError::Pattern { .. }));
    }
}
