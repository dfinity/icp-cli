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

/// This machine's filesystem, held to one directory, for a project that is
/// not the user's own tree: one handed over by someone else.
///
/// The root is opened once, as a directory handle, and every read and write
/// is made beneath that handle through [`cap_std`]: the operating system
/// resolves the path and refuses one that leaves the directory, by `..`, by
/// an absolute path or by a symlink, in the same step that opens the file.
/// Where the kernel offers that as one call (`openat2` on Linux) it is one
/// call; elsewhere `cap_std` walks the path a handle at a time.
///
/// On top of that, every path must reach what it names through no symlink.
/// A relative path is joined to the root, `.` is dropped, `..` is applied,
/// and a path that comes to rest outside the root is refused with
/// [`OutsideRoot`]. Each component below the root is looked at as it is on
/// disk, and a path that passes through a symlink is refused with
/// [`ThroughLink`], wherever the link leads, even where a later `..` steps
/// back out of it. The predicates answer `false` for a refused path, and
/// scratch directories are made under the root.
///
/// The two differ in what a race can do to them. A link swapped in between
/// the look and the read is followed, since the look is a separate step, but
/// only to somewhere under the root: the handle does not let it lead out.
///
/// This confines what is read and written through it, and nothing else: a
/// `script` build step in a manifest still runs any command, so whoever
/// holds a tree they do not trust must also refuse such steps or sandbox them.
///
/// The loader reads every path through the [`FileSystem`] it is given, so
/// this holds all of them. A local recipe template is the exception: the
/// [`Resolve`](crate::canister::recipe::Resolve) implementation reads it, and
/// must read it through this filesystem or refuse it.
///
/// Known issue: a predicate can only answer `true` or `false`, so the reason
/// a path was refused is lost there, and a caller that checks before it
/// reads reports a refused path as missing. The loader does this for a
/// canister directory and for a dependency's manifest: `canisters:
/// [../elsewhere]` fails as "not found" rather than as outside the root.
///
/// The root is absolute, and is matched as the caller spelled it, not as it
/// canonicalizes. It and everything above it are the caller's, and may be
/// reached through a link; only what lies below is held to shape.
#[cfg(feature = "host")]
#[derive(Debug, Clone)]
pub struct Confined {
    root: PathBuf,
    dir: std::sync::Arc<cap_std::fs::Dir>,
}

/// A root a [`Confined`] filesystem could not be held to.
#[cfg(feature = "host")]
#[derive(Debug, Snafu)]
pub enum RootError {
    #[snafu(display(
        "'{root}' is not an absolute path, which the directory a project is confined to must be"
    ))]
    NotAbsolute { root: PathBuf },

    #[snafu(display("failed to open '{root}', the directory to confine the project to"))]
    Open {
        source: std::io::Error,
        root: PathBuf,
    },
}

/// A path a [`Confined`] filesystem was asked for that is not under its root.
#[cfg(feature = "host")]
#[derive(Debug, Snafu)]
#[snafu(display("'{path}' is outside '{root}', the directory this project is confined to"))]
pub struct OutsideRoot {
    pub path: PathBuf,
    pub root: PathBuf,
}

/// A path a [`Confined`] filesystem was asked for that passes through a
/// symlink.
#[cfg(feature = "host")]
#[derive(Debug, Snafu)]
#[snafu(display(
    "'{path}' passes through '{link}', which is a symlink, and every path in this project must \
     name a regular file or directory under its root"
))]
pub struct ThroughLink {
    pub path: PathBuf,
    pub link: PathBuf,
}

/// An operation beneath a [`Confined`] filesystem's root failed.
#[cfg(feature = "host")]
#[derive(Debug, Snafu)]
pub enum ConfinedError {
    #[snafu(display("failed to read '{path}'"))]
    Read {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display(
        "'{path}' is not a regular file, which is all that is read from this project"
    ))]
    NotAFile { path: PathBuf },

    #[snafu(display("failed to write '{path}'"))]
    Write {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to create directory '{path}'"))]
    CreateDir {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to copy '{from}' to '{to}'"))]
    Copy {
        source: std::io::Error,
        from: PathBuf,
        to: PathBuf,
    },

    #[snafu(display("failed to list '{path}'"))]
    ReadDir {
        source: std::io::Error,
        path: PathBuf,
    },
}

#[cfg(feature = "host")]
impl Confined {
    /// Confine the host filesystem to `root`, which must be absolute and a
    /// directory that is there to open.
    pub fn new(root: &Path) -> Result<Self, RootError> {
        snafu::ensure!(root.is_absolute(), NotAbsoluteSnafu { root });
        let dir = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .context(OpenSnafu { root })?;
        Ok(Self {
            root: root.to_path_buf(),
            dir: std::sync::Arc::new(dir),
        })
    }

    /// The directory everything is held to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `path` lies under the root, as a real path: refused if it would
    /// come to rest outside, or passes through a symlink on the way.
    ///
    /// What a caller that needs a path for something that does not read
    /// through the trait — a runtime mounting a directory —
    /// asks for. Such a caller opens the path itself, after this has looked,
    /// so it has this filesystem's rule but not the handle's guarantee.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, FsError> {
        Ok(self.root.join(self.beneath(path)?))
    }

    /// `path` relative to the root, with `.` dropped and `..` applied:
    /// refused if it would come to rest outside the root, or passes through
    /// a symlink on the way. Empty for the root itself.
    fn beneath(&self, path: &Path) -> Result<PathBuf, FsError> {
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
        // Each component is looked at as it is on disk the moment it is
        // named, before any `..` after it is applied: the host would follow a
        // link and take `..` from where it leads, so a path that so much as
        // mentions one is refused, dangling or not.
        let mut beneath = PathBuf::new();
        for component in relative.components() {
            match component {
                Utf8Component::Normal(part) => {
                    beneath.push(part);
                    if self
                        .dir
                        .symlink_metadata(&beneath)
                        .is_ok_and(|m| m.file_type().is_symlink())
                    {
                        return Err(FsError::new(ThroughLink {
                            path: path.to_path_buf(),
                            link: self.root.join(beneath),
                        }));
                    }
                }
                Utf8Component::CurDir => {}
                Utf8Component::ParentDir => {
                    if !beneath.pop() {
                        return Err(outside());
                    }
                }
                Utf8Component::RootDir | Utf8Component::Prefix(_) => return Err(outside()),
            }
        }
        Ok(beneath)
    }

    /// As [`at`](Self::at), for a path about to be read: refused unless what
    /// is there is a regular file. Opening a pipe nobody writes to waits
    /// forever, so the question is asked before the open. A path with
    /// nothing at it goes through, for the read to report as missing.
    fn file_at(&self, path: &Path) -> Result<PathBuf, FsError> {
        let at = self.at(path)?;
        if self.dir.metadata(&at).is_ok_and(|m| !m.is_file()) {
            return Err(FsError::new(NotAFileSnafu { path }.build()));
        }
        Ok(at)
    }

    /// As [`beneath`](Self::beneath), spelled the way the directory handle
    /// takes it: it has no name for itself but `.`.
    fn at(&self, path: &Path) -> Result<PathBuf, FsError> {
        let beneath = self.beneath(path)?;
        Ok(if beneath.as_str().is_empty() {
            PathBuf::from(".")
        } else {
            beneath
        })
    }
}

#[cfg(feature = "host")]
#[async_trait]
impl FileSystem for Confined {
    async fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        self.dir
            .read(self.file_at(path)?)
            .context(ReadSnafu { path })
            .map_err(FsError::new)
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, FsError> {
        self.dir
            .read_to_string(self.file_at(path)?)
            .context(ReadSnafu { path })
            .map_err(FsError::new)
    }

    async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
        self.dir
            .write(self.at(path)?, contents)
            .context(WriteSnafu { path })
            .map_err(FsError::new)
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), FsError> {
        self.dir
            .create_dir_all(self.at(path)?)
            .context(CreateDirSnafu { path })
            .map_err(FsError::new)
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        self.dir
            .copy(self.at(from)?, &self.dir, self.at(to)?)
            .map(|_| ())
            .context(CopySnafu { from, to })
            .map_err(FsError::new)
    }

    async fn exists(&self, path: &Path) -> bool {
        self.at(path).is_ok_and(|at| self.dir.exists(at))
    }

    async fn is_file(&self, path: &Path) -> bool {
        self.at(path)
            .is_ok_and(|at| self.dir.metadata(at).is_ok_and(|m| m.is_file()))
    }

    async fn is_dir(&self, path: &Path) -> bool {
        self.at(path)
            .is_ok_and(|at| self.dir.metadata(at).is_ok_and(|m| m.is_dir()))
    }

    /// A name that is not UTF-8 is skipped rather than being an error, as the
    /// host's own listing skips it.
    async fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FsError> {
        let at = self.at(path)?;
        let list = || -> std::io::Result<Vec<PathBuf>> {
            let mut out = Vec::new();
            for entry in self.dir.read_dir(at)? {
                if let Ok(name) = entry?.file_name().into_string() {
                    out.push(path.join(name));
                }
            }
            out.sort();
            Ok(out)
        };
        list().context(ReadDirSnafu { path }).map_err(FsError::new)
    }

    /// The path under the root as the caller spelled the root, so that what
    /// comes back can be asked for again.
    async fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        let within = self.dir.canonicalize(self.at(path).ok()?).ok()?;
        Some(self.root.join(PathBuf::from_path_buf(within).ok()?))
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
    /// relative one is joined, `.` and `..` are applied, an absolute one is
    /// taken only under the root, and none may pass through a symlink.
    #[cfg(feature = "host")]
    mod confined {
        use super::*;

        fn root() -> (camino_tempfile::Utf8TempDir, Confined) {
            let dir = camino_tempfile::Utf8TempDir::new().expect("temp dir");
            std::fs::create_dir_all(dir.path().join("project/canisters")).expect("mkdir");
            std::fs::write(dir.path().join("project/icp.yaml"), b"canisters: []").expect("write");
            std::fs::write(dir.path().join("project/canisters/a.wasm"), b"wasm").expect("write");
            std::fs::write(dir.path().join("outside.txt"), b"secret").expect("write");
            let fs = Confined::new(&dir.path().join("project")).expect("absolute");
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
                    Some(root.join("canisters/a.wasm")),
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

        /// A temp dir on macOS is reached through a symlink (`/var`), so a
        /// root compared by what it canonicalizes to would refuse every
        /// absolute path spelled the way the caller got it.
        #[cfg(unix)]
        #[tokio::test]
        async fn the_root_is_matched_as_the_caller_spells_it() {
            let (dir, _) = root();
            std::os::unix::fs::symlink(dir.path().join("project"), dir.path().join("alias"))
                .expect("symlink");
            let spelled = dir.path().join("alias");
            let fs = Confined::new(&spelled).expect("absolute");
            assert_eq!(
                fs.read(&spelled.join("icp.yaml")).await.unwrap(),
                b"canisters: []"
            );
            assert_eq!(
                fs.read(Path::new("icp.yaml")).await.unwrap(),
                b"canisters: []"
            );
            assert!(fs.is_dir(&spelled).await);
            // What canonicalizing gives back is spelled the same way, so it
            // can be asked for again.
            let canonical = fs.canonicalize(Path::new("icp.yaml")).await.unwrap();
            assert_eq!(canonical, spelled.join("icp.yaml"));
            assert_eq!(fs.read(&canonical).await.unwrap(), b"canisters: []");
        }

        #[test]
        fn a_root_that_is_not_absolute_is_refused() {
            let err = Confined::new(Path::new("project")).unwrap_err();
            assert!(err.to_string().contains("not an absolute path"), "{err}");
        }

        #[test]
        fn a_root_that_is_not_a_directory_to_open_is_refused() {
            let (dir, _) = root();
            for root in ["nowhere", "project/icp.yaml"] {
                let err = Confined::new(&dir.path().join(root)).unwrap_err();
                assert!(err.to_string().contains("failed to open"), "{root}: {err}");
            }
        }

        /// A path through a symlink is refused wherever the link leads: out
        /// of the root, back inside it, or nowhere. A link in the middle of
        /// a path counts as much as one at its end.
        #[cfg(unix)]
        #[tokio::test]
        async fn a_path_through_a_symlink_is_refused_wherever_it_leads() {
            use std::os::unix::fs::symlink;
            let (dir, fs) = root();
            let project = fs.root().to_path_buf();
            symlink(dir.path().join("outside.txt"), project.join("out.txt")).expect("symlink");
            symlink(project.join("icp.yaml"), project.join("in.yaml")).expect("symlink");
            symlink(project.join("nowhere"), project.join("dangling")).expect("symlink");
            symlink(project.join("canisters"), project.join("dir-link")).expect("symlink");

            for (spelling, link) in [
                ("out.txt", "out.txt"),
                ("in.yaml", "in.yaml"),
                ("dangling", "dangling"),
                ("dir-link/a.wasm", "dir-link"),
                ("./dir-link/../dir-link/a.wasm", "dir-link"),
                // The host would take `..` from where the link leads; the
                // spelling alone would cancel the link out and find the file.
                ("dir-link/../canisters/a.wasm", "dir-link"),
                ("dangling/../icp.yaml", "dangling"),
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
                assert!(
                    fs.canonicalize(Path::new(spelling)).await.is_none(),
                    "{spelling}"
                );
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
        }

        /// The look for a link and the read are two steps, and the handle is
        /// what holds if the first is got past: asked directly, it follows a
        /// link that stays under the root and refuses one that leaves.
        #[cfg(unix)]
        #[test]
        fn the_handle_refuses_a_link_out_of_the_root_by_itself() {
            use std::os::unix::fs::symlink;
            let (dir, fs) = root();
            let project = fs.root().to_path_buf();
            symlink(dir.path().join("outside.txt"), project.join("out.txt")).expect("symlink");
            symlink("../outside.txt", project.join("out-relative.txt")).expect("symlink");
            symlink("icp.yaml", project.join("in.yaml")).expect("symlink");

            assert!(fs.dir.read("out.txt").is_err());
            assert!(fs.dir.read("out-relative.txt").is_err());
            assert!(fs.dir.read("canisters/../../outside.txt").is_err());
            assert!(fs.dir.write("out.txt", b"x").is_err());
            assert_eq!(fs.dir.read("in.yaml").unwrap(), b"canisters: []");
            assert_eq!(
                std::fs::read(dir.path().join("outside.txt")).unwrap(),
                b"secret"
            );
        }

        /// A pipe passes for a path under the root through no link, and a
        /// read of one nobody writes to would never come back.
        #[cfg(unix)]
        #[tokio::test]
        async fn a_read_of_what_is_not_a_regular_file_is_refused_before_it_opens() {
            let (_dir, fs) = root();
            let made = std::process::Command::new("mkfifo")
                .arg(fs.root().join("pipe"))
                .status()
                .expect("mkfifo");
            assert!(made.success(), "mkfifo");
            for path in ["pipe", "canisters"] {
                let err = fs.read(Path::new(path)).await.expect_err(path).to_string();
                assert!(err.contains("not a regular file"), "{path}: {err}");
                let err = fs
                    .read_to_string(Path::new(path))
                    .await
                    .expect_err(path)
                    .to_string();
                assert!(err.contains("not a regular file"), "{path}: {err}");
            }
            // What is not there is still reported as that.
            let err = fs.read(Path::new("gone")).await.unwrap_err().to_string();
            assert!(err.contains("failed to read"), "{err}");
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
