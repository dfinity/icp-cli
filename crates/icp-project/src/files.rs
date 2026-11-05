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

use std::collections::HashSet;

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

/// This machine's filesystem, held to one directory by the spelling of each
/// path alone.
///
/// Every path is resolved against the root before the disk sees it: a
/// relative path is joined to the root, `.` is dropped, `..` is applied, and
/// a path that comes to rest outside the root — by climbing above it, or by
/// being absolute somewhere else — is refused with [`OutsideRoot`]. The
/// predicates answer `false` for such a path, as they do for any other
/// failure, and scratch directories are made under the root, so what a build
/// writes and reads back stays inside.
///
/// For a project whose files are not the user's own tree: a bundle handed
/// over by someone else, which `icp project bundle` wrote with every path
/// held to the project it came from, but which the loader would otherwise
/// take at its word. The loader resolves every path it reads itself through
/// the [`FileSystem`] it is given, so confining that one seam holds all of
/// them — the canister directories, the manifests in them, the argument and
/// variable files, the dependency projects — without the loader enumerating
/// its fields. A local recipe template is the one file the loader has read
/// for it rather than reading: the [`Resolve`](crate::canister::recipe::Resolve)
/// implementation fetches it, and one holding a tree it does not trust must
/// confine or refuse that itself.
///
/// The check is lexical, so it holds only for a tree without symlinks. That
/// is the shape a bundle has: `icp project bundle` writes an archive of
/// regular files and directories, and a consumer unpacking one it does not
/// trust refuses any entry that is not, so nothing in the tree can lead
/// elsewhere. A tree that does carry symlinks is followed as the host
/// follows them, and that is deliberate — the user's own project may link
/// wherever it likes. The tree is also taken to be the caller's own while
/// the project is loaded, with nothing else rewriting it underneath.
///
/// Paths are matched against the root as the caller spelled it, not as it
/// canonicalizes, so a caller that names the root through a symlink (macOS
/// keeps `/var` as a link to `/private/var`) or by its short name (Windows'
/// `RUNNER~1`) is understood as long as it keeps to the same spelling.
///
/// A tree that nobody has held to that shape — a directory on disk that
/// someone else wrote, rather than an archive unpacked by a consumer that
/// refused its links — is asked for through
/// [`refusing_links`](Self::refusing_links) instead. Each component of a
/// path below the root is then looked at as it is on disk, and a path that
/// passes through a symlink anywhere under the root is refused with
/// [`ThroughLink`], wherever the link leads: one that points inside the root
/// is as unwelcome as one that points out, since the rule is the shape of the
/// tree rather than where any one link goes. The root itself and everything
/// above it are the caller's, and are not looked at.
#[cfg(feature = "host")]
#[derive(Debug, Clone)]
pub struct Confined {
    root: PathBuf,
    links: Links,
}

/// What a [`Confined`] filesystem does with a symlink under its root.
#[cfg(feature = "host")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Links {
    /// Follow it, as the host does. For a tree that has none, or whose links
    /// are the caller's own.
    Follow,
    /// Refuse any path that passes through one.
    Refuse,
}

/// A path a [`Confined`] filesystem was asked for that is not under its root.
#[cfg(feature = "host")]
#[derive(Debug, Snafu)]
#[snafu(display("'{path}' is outside '{root}', the directory this project is confined to"))]
pub struct OutsideRoot {
    pub path: PathBuf,
    pub root: PathBuf,
}

/// A path a [`Confined`] filesystem that refuses links was asked for that
/// passes through one.
#[cfg(feature = "host")]
#[derive(Debug, Snafu)]
#[snafu(display(
    "'{path}' passes through '{link}', which is a symlink, and this project may not have any"
))]
pub struct ThroughLink {
    pub path: PathBuf,
    pub link: PathBuf,
}

#[cfg(feature = "host")]
impl Confined {
    /// Confine the host filesystem to `root`, following any symlink under it
    /// as the host does.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            links: Links::Follow,
        }
    }

    /// Confine the host filesystem to `root`, and refuse any path that passes
    /// through a symlink under it.
    pub fn refusing_links(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            links: Links::Refuse,
        }
    }

    /// The directory everything is held to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `path` lies under the root: joined to the root if relative,
    /// with `.` dropped and `..` applied, and refused if it would come to
    /// rest outside — or, for a filesystem that refuses links, if it passes
    /// through one on the way there. What a caller that needs a real path
    /// for something that does not read through the trait — a runtime
    /// mounting a directory — asks for.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, FsError> {
        let outside = || {
            FsError::new(OutsideRoot {
                path: path.to_path_buf(),
                root: self.root.clone(),
            })
        };
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root).map_err(|_| outside())?
        } else {
            path
        };
        let mut resolved = self.root.clone();
        for component in relative.components() {
            match component {
                Utf8Component::Normal(part) => resolved.push(part),
                Utf8Component::CurDir => {}
                Utf8Component::ParentDir => {
                    if resolved == self.root || !resolved.pop() {
                        return Err(outside());
                    }
                }
                Utf8Component::RootDir | Utf8Component::Prefix(_) => return Err(outside()),
            }
        }
        if self.links == Links::Refuse {
            self.refuse_links(path, &resolved)?;
        }
        Ok(resolved)
    }

    /// Refuse `resolved` if any component of it below the root is a symlink.
    ///
    /// Each component is looked at as it is on disk rather than through
    /// what it points at, so a link is seen wherever it leads, and a dangling
    /// one is seen too. The walk stops at the first component that is not
    /// there: nothing beyond it can be a link, and whether the path exists
    /// is for the operation itself to find out and report.
    fn refuse_links(&self, path: &Path, resolved: &Path) -> Result<(), FsError> {
        let mut at = self.root.clone();
        for component in resolved
            .strip_prefix(&self.root)
            .expect("resolved under the root")
            .components()
        {
            at.push(component.as_str());
            match std::fs::symlink_metadata(&at) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(FsError::new(ThroughLink {
                        path: path.to_path_buf(),
                        link: at,
                    }));
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Ok(())
    }
}

#[cfg(feature = "host")]
#[async_trait]
impl FileSystem for Confined {
    async fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        HostFileSystem.read(&self.resolve(path)?).await
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, FsError> {
        HostFileSystem.read_to_string(&self.resolve(path)?).await
    }

    async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
        HostFileSystem.write(&self.resolve(path)?, contents).await
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), FsError> {
        HostFileSystem.create_dir_all(&self.resolve(path)?).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        HostFileSystem
            .copy(&self.resolve(from)?, &self.resolve(to)?)
            .await
    }

    async fn exists(&self, path: &Path) -> bool {
        match self.resolve(path) {
            Ok(path) => HostFileSystem.exists(&path).await,
            Err(_) => false,
        }
    }

    async fn is_file(&self, path: &Path) -> bool {
        match self.resolve(path) {
            Ok(path) => HostFileSystem.is_file(&path).await,
            Err(_) => false,
        }
    }

    async fn is_dir(&self, path: &Path) -> bool {
        match self.resolve(path) {
            Ok(path) => HostFileSystem.is_dir(&path).await,
            Err(_) => false,
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FsError> {
        HostFileSystem.read_dir(&self.resolve(path)?).await
    }

    async fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        HostFileSystem.canonicalize(&self.resolve(path).ok()?).await
    }

    async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
        let dir = camino_tempfile::Builder::new()
            .prefix(".scratch-")
            .tempdir_in(&self.root)
            .map_err(FsError::new)?;
        Ok(Box::new(HostScratch(dir)))
    }
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
/// A component with nothing to match against is instead resolved the way
/// joining it onto `base` would resolve it: `..` climbs, and an absolute
/// pattern starts from its own root with `base` dropped. A component with no
/// metacharacter is joined too, since it names one path rather than describing
/// a set of them, so a pattern with no metacharacters at all names the same
/// path here as `base.join(pattern)` does.
///
/// Only paths that exist are returned, and each directory's entries are listed
/// in sorted order, so the result is the same on every run. `**` descends into
/// each directory once, identified rather than spelled, so a symlink cycle
/// terminates.
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
            // directory rather than matching one: the directory above `dir`,
            // which is there only where `dir` itself is. Asking about `dir`
            // rather than about the joined path is what settles that — Windows
            // resolves a trailing `..` by spelling rather than by lookup, and
            // answers for `nowhere/..` as readily as for a path that is there.
            Utf8Component::ParentDir => {
                let mut next = Vec::new();
                for dir in &frontier {
                    if files.is_dir(dir).await {
                        next.push(dir.join(".."));
                    }
                }
                frontier = next;
            }

            // `**` matches zero or more directories, so every reachable
            // directory — including the ones already in hand — carries forward.
            //
            // Descending is keyed on a directory's identity rather than its
            // path, because `is_dir` answers through symlinks: a link back to
            // an ancestor is one directory reachable under endlessly many
            // paths, and descending each of them yields the same files again
            // under an ever longer name, until the implementation refuses to
            // resolve any more — or, where nothing refuses, never. So the
            // first spelling of a directory is the one that matches and is
            // descended, and a later one matches nothing at all: the files
            // under it are the same files, and reporting them twice would make
            // one canister look like two.
            //
            // An implementation that cannot establish identity is descended
            // anyway: it has no links to come back around, or it would be able
            // to resolve them.
            Utf8Component::Normal("**") => {
                let mut seen = HashSet::new();
                let mut reached = frontier.clone();
                for dir in &reached {
                    if let Some(id) = files.canonicalize(dir).await {
                        seen.insert(id);
                    }
                }

                let mut stack = frontier;
                while let Some(dir) = stack.pop() {
                    for entry in list(files, &dir, pattern).await? {
                        if files.is_dir(&entry).await
                            && files
                                .canonicalize(&entry)
                                .await
                                .is_none_or(|id| seen.insert(id))
                        {
                            reached.push(entry.clone());
                            stack.push(entry);
                        }
                    }
                }
                reached.sort();
                reached.dedup();
                frontier = reached;
            }

            // A component with no metacharacter matches the single name it
            // spells, so it too is joined rather than looked for in a listing.
            // Windows spells one file several ways — a case the directory
            // entry does not use, or a short name like `RUNNER~1` — and each
            // of them opens the file, so looking for the spelling among the
            // entries would turn a pattern that names a path into one that
            // matches nothing.
            Utf8Component::Normal(component) if !component.contains(['*', '?', '[']) => {
                let mut next = Vec::new();
                for dir in &frontier {
                    let entry = dir.join(component);
                    if files.exists(&entry).await {
                        next.push(entry);
                    }
                }
                frontier = next;
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

    /// Every spelling of a path resolves under the root or is refused: a
    /// relative one is joined, `.` and `..` are applied, and an absolute one
    /// is taken only under the root. By its spelling alone: a symlink is the
    /// extractor's business, not this type's.
    #[cfg(feature = "host")]
    mod confined {
        use super::*;

        fn root() -> (camino_tempfile::Utf8TempDir, Confined) {
            let dir = camino_tempfile::Utf8TempDir::new().expect("temp dir");
            std::fs::create_dir_all(dir.path().join("project/canisters")).expect("mkdir");
            std::fs::write(dir.path().join("project/icp.yaml"), b"canisters: []").expect("write");
            std::fs::write(dir.path().join("project/canisters/a.wasm"), b"wasm").expect("write");
            std::fs::write(dir.path().join("outside.txt"), b"secret").expect("write");
            let fs = Confined::new(&dir.path().join("project"));
            (dir, fs)
        }

        #[tokio::test]
        async fn paths_resolve_under_the_root_by_any_spelling_inside_it() {
            let (_dir, fs) = root();
            let root = fs.root().to_path_buf();
            for spelling in [
                "canisters/a.wasm",
                "./canisters/a.wasm",
                "canisters/../canisters/a.wasm",
                root.join("canisters/a.wasm").as_str(),
            ] {
                assert_eq!(fs.read(Path::new(spelling)).await.expect(spelling), b"wasm");
                assert!(fs.is_file(Path::new(spelling)).await, "{spelling}");
                assert_eq!(
                    fs.canonicalize(Path::new(spelling)).await,
                    HostFileSystem
                        .canonicalize(&root.join("canisters/a.wasm"))
                        .await,
                    "{spelling}"
                );
            }
            assert!(fs.is_dir(Path::new("canisters")).await);
            assert!(fs.is_dir(&root).await);
            assert_eq!(
                fs.read_dir(&root).await.expect("list"),
                vec![root.join("canisters"), root.join("icp.yaml")]
            );
            assert!(
                fs.resolve(Path::new("made/later"))
                    .unwrap()
                    .starts_with(&root)
            );
        }

        #[tokio::test]
        async fn paths_outside_the_root_are_refused_however_spelled() {
            let (dir, fs) = root();
            let outside = dir.path().join("outside.txt");
            assert!(
                outside.is_file(),
                "the file exists, for the host filesystem to read"
            );
            for spelling in [
                "../outside.txt",
                "canisters/../../outside.txt",
                outside.as_str(),
                "/etc/hostname",
                "../../../../../../etc/hostname",
            ] {
                let err = fs
                    .read(Path::new(spelling))
                    .await
                    .expect_err(spelling)
                    .to_string();
                assert!(err.contains("is outside"), "{spelling}: {err}");
                assert!(!fs.exists(Path::new(spelling)).await, "{spelling}");
                assert!(!fs.is_file(Path::new(spelling)).await, "{spelling}");
                assert!(
                    fs.canonicalize(Path::new(spelling)).await.is_none(),
                    "{spelling}"
                );
                assert!(fs.resolve(Path::new(spelling)).is_err(), "{spelling}");
            }
            // Nor may anything be written there.
            let err = fs
                .write(Path::new("../planted"), b"x")
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("is outside"), "{err}");
            assert!(!dir.path().join("planted").exists());
        }

        /// What broke on macOS and Windows: the temp dir is reached through
        /// a symlink (`/var` to `/private/var`) or a short name (`RUNNER~1`),
        /// so a root compared by what it canonicalizes to refused every
        /// absolute path spelled the way the caller got it.
        #[cfg(unix)]
        #[tokio::test]
        async fn the_root_is_matched_as_the_caller_spells_it() {
            let (dir, _) = root();
            std::os::unix::fs::symlink(dir.path().join("project"), dir.path().join("alias"))
                .expect("symlink");
            let spelled = dir.path().join("alias");
            let fs = Confined::new(&spelled);
            assert_eq!(
                fs.read(&spelled.join("icp.yaml")).await.unwrap(),
                b"canisters: []"
            );
            assert_eq!(
                fs.read(Path::new("icp.yaml")).await.unwrap(),
                b"canisters: []"
            );
            assert!(fs.is_dir(&spelled).await);
        }

        /// The check is lexical: a symlink is followed as the host follows
        /// it. A tree someone else handed over must have had its links
        /// refused on the way in; the user's own tree may link where it
        /// likes.
        #[cfg(unix)]
        #[tokio::test]
        async fn a_symlink_is_followed_as_the_host_follows_it() {
            let (dir, fs) = root();
            std::os::unix::fs::symlink(dir.path().join("outside.txt"), fs.root().join("link.txt"))
                .expect("symlink");
            assert_eq!(fs.read(Path::new("link.txt")).await.unwrap(), b"secret");
        }

        /// Asked to refuse links, the filesystem refuses a path through one
        /// wherever it leads: out of the root, back inside it, or nowhere.
        /// A link in the middle of a path counts as much as one at its end.
        #[cfg(unix)]
        #[tokio::test]
        async fn refusing_links_refuses_a_path_through_one_wherever_it_leads() {
            use std::os::unix::fs::symlink;
            let (dir, _) = root();
            let project = dir.path().join("project");
            symlink(dir.path().join("outside.txt"), project.join("out.txt")).expect("symlink");
            symlink(project.join("icp.yaml"), project.join("in.yaml")).expect("symlink");
            symlink(project.join("nowhere"), project.join("dangling")).expect("symlink");
            symlink(project.join("canisters"), project.join("dir-link")).expect("symlink");
            let fs = Confined::refusing_links(&project);

            for (spelling, link) in [
                ("out.txt", "out.txt"),
                ("in.yaml", "in.yaml"),
                ("dangling", "dangling"),
                ("dir-link/a.wasm", "dir-link"),
                ("./dir-link/../dir-link/a.wasm", "dir-link"),
            ] {
                let err = fs
                    .read(Path::new(spelling))
                    .await
                    .expect_err(spelling)
                    .to_string();
                assert!(err.contains("passes through"), "{spelling}: {err}");
                assert!(err.contains(link), "{spelling}: {err}");
                assert!(!fs.exists(Path::new(spelling)).await, "{spelling}");
                assert!(!fs.is_file(Path::new(spelling)).await, "{spelling}");
                assert!(!fs.is_dir(Path::new(spelling)).await, "{spelling}");
                assert!(fs.resolve(Path::new(spelling)).is_err(), "{spelling}");
            }
            // Nor may a link be written through.
            let err = fs
                .write(Path::new("out.txt"), b"x")
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("passes through"), "{err}");
            assert_eq!(
                std::fs::read(dir.path().join("outside.txt")).unwrap(),
                b"secret"
            );

            // Regular files and directories are read as before, and a path
            // that is not there yet resolves, since nothing on it is a link.
            assert_eq!(
                fs.read(Path::new("canisters/a.wasm")).await.unwrap(),
                b"wasm"
            );
            assert!(fs.is_dir(Path::new("canisters")).await);
            assert!(fs.resolve(Path::new("made/later")).is_ok());
            assert_eq!(
                fs.read_dir(&project).await.expect("list").len(),
                6,
                "listing shows the links; reading through them is what is refused"
            );
        }

        /// The root itself may be reached through a link: it is the caller's
        /// spelling, and only what lies below it is held to shape.
        #[cfg(unix)]
        #[tokio::test]
        async fn refusing_links_still_takes_the_root_as_the_caller_spells_it() {
            let (dir, _) = root();
            std::os::unix::fs::symlink(dir.path().join("project"), dir.path().join("alias"))
                .expect("symlink");
            let spelled = dir.path().join("alias");
            let fs = Confined::refusing_links(&spelled);
            assert_eq!(
                fs.read(&spelled.join("icp.yaml")).await.unwrap(),
                b"canisters: []"
            );
            assert_eq!(
                fs.read(Path::new("canisters/a.wasm")).await.unwrap(),
                b"wasm"
            );
        }

        #[tokio::test]
        async fn scratch_is_made_under_the_root() {
            let (_dir, fs) = root();
            let scratch = fs.scratch_dir().await.expect("scratch");
            assert!(scratch.path().starts_with(fs.root()));
            let out = scratch.path().join("out/module.wasm");
            fs.create_dir_all(&scratch.path().join("out"))
                .await
                .expect("mkdir");
            fs.write(&out, b"built").await.expect("write");
            assert_eq!(fs.read(&out).await.unwrap(), b"built");
            let path = scratch.path().to_path_buf();
            drop(scratch);
            assert!(!path.exists());
        }
    }

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

    /// Windows spells one directory several ways, and a spelling the listing
    /// does not use still names it.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_literal_component_need_not_be_spelled_as_the_listing_spells_it() {
        let d = tree(&["canisters/a/canister.yaml"]);
        assert_eq!(expand(d.path(), "CANISTERS/a").await, ["CANISTERS/a"]);
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

    /// A directory that links back to an ancestor is reachable under endlessly
    /// many paths. `**` still finishes, and reports each file once.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_double_star_does_not_follow_a_symlink_cycle() {
        let d = tree(&["services/a/one.yaml"]);
        std::os::unix::fs::symlink(d.path().join("services"), d.path().join("services/a/loop"))
            .expect("symlink");
        assert_eq!(
            expand(d.path(), "services/**/*.yaml").await,
            ["services/a/one.yaml"]
        );
    }

    /// A name that is not UTF-8 cannot be a project path, so it is left out of
    /// a listing rather than failing it. Only Linux lets such a name be written
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_name_that_is_not_utf8_is_skipped_rather_than_an_error() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let d = tree(&["packages/a/canister.yaml"]);
        let stray = d
            .path()
            .join("packages")
            .into_std_path_buf()
            .join(OsStr::from_bytes(b"caf\xe9"));
        std::fs::create_dir(&stray).expect("mkdir");
        std::fs::write(stray.join("canister.yaml"), b"").expect("write");

        assert_eq!(expand(d.path(), "packages/*").await, ["packages/a"]);
        assert_eq!(
            expand(d.path(), "packages/**/canister.yaml").await,
            ["packages/a/canister.yaml"]
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
