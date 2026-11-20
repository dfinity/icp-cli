#[cfg(feature = "host")]
use std::sync::Arc;
#[cfg(any(test, feature = "test-util"))]
use std::{collections::HashMap, sync::Mutex};

use crate::prelude::*;
#[cfg(any(test, feature = "host", feature = "test-util"))]
use crate::store_id::StoreCause;
#[cfg(feature = "host")]
use crate::{
    CACHE_DIR, ICP_BASE,
    fs::{
        json,
        lock::{DirectoryStructureLock, PathsAccess},
        read, remove_file, write,
    },
    manifest::ProjectRootLocate,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use snafu::{Snafu, ensure};

#[async_trait]
/// Trait for accessing and managing canister build artifacts.
pub trait Access: Sync + Send {
    /// Save a canister artifact (WASM) to the store.
    async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError>;

    /// Make the file at `path` a canister's artifact, where it lies.
    ///
    /// Nothing is copied: the store keeps `path` and `sha256`, the SHA-256 of
    /// the file's contents now in lowercase hex, and [`lookup`](Self::lookup)
    /// reads the file itself. So a module any number of canisters are built
    /// from is on disk once, however many of them there are.
    ///
    /// The file stays its owner's. The store never writes, moves or removes
    /// it, and nothing stops it changing or going away after this returns:
    /// `lookup` then fails, naming the file, rather than hand out a module
    /// other than the one that was built.
    async fn link(&self, name: &str, path: &Path, sha256: &str) -> Result<(), SaveError>;

    /// Lookup a canister artifact (WASM) from the store.
    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupArtifactError>;
}

#[derive(Debug, Snafu)]
pub enum SaveError {
    #[snafu(display(
        "canister '{name}' encodes to a {len}-byte artifact filename, exceeding the 255-byte \
         filesystem limit; shorten the dependency path or canister name"
    ))]
    SaveNameTooLong { name: String, len: usize },

    /// The store could not keep the artifact. What a store is made of is the
    /// implementation's business, so the cause is carried whole.
    #[snafu(display("failed to store the build artifact for canister '{name}'"))]
    SaveStore {
        source: crate::store_id::StoreCause,
        name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum LookupArtifactError {
    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },

    #[snafu(display(
        "canister '{name}' encodes to a {len}-byte artifact filename, exceeding the 255-byte \
         filesystem limit; shorten the dependency path or canister name"
    ))]
    LookupNameTooLong { name: String, len: usize },

    /// As [`SaveError::SaveStore`].
    #[snafu(display("failed to read the build artifact for canister '{name}'"))]
    LookupStore {
        source: crate::store_id::StoreCause,
        name: String,
    },

    /// The file a [linked](Access::link) artifact names is not there to read.
    #[snafu(display(
        "canister '{name}' was built from the module at '{path}', which can no longer be read; \
         build the canister again"
    ))]
    LookupLinkedRead {
        source: crate::store_id::StoreCause,
        name: String,
        path: PathBuf,
    },

    /// The file a [linked](Access::link) artifact names is not the one that
    /// was linked.
    #[snafu(display(
        "canister '{name}' was built from the module at '{path}', which has changed since: its \
         sha256 was {expected} and is now {actual}; build the canister again"
    ))]
    LookupLinkedChanged {
        name: String,
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

/// Hold what was read from a [linked](Access::link) file to the hash it was
/// linked with, and hand it over only if it is still that module.
///
/// For any [`Access`] to finish a lookup with: the file is someone else's, so
/// reading it back says nothing about whether it is what was built.
pub fn verify_linked(
    name: &str,
    path: &Path,
    expected: &str,
    wasm: Vec<u8>,
) -> Result<Vec<u8>, LookupArtifactError> {
    let actual = hex::encode(Sha256::digest(&wasm));
    ensure!(
        actual == expected,
        LookupLinkedChangedSnafu {
            name,
            path,
            expected,
            actual,
        }
    );
    Ok(wasm)
}

#[cfg(feature = "host")]
pub struct ArtifactStore {
    project_root_locate: Arc<dyn ProjectRootLocate>,
}

#[cfg(feature = "host")]
pub struct ArtifactPaths {
    dir: PathBuf,
}

/// Encode a canister name into a single filename-safe segment.
///
/// Canister names may be namespaced store keys containing `/` and `:` (imported
/// dependency canisters, e.g. `vendor/openemail:backend`), which are not valid
/// filename characters on every platform. Percent-encoding the unsafe set keeps
/// the mapping reversible and collision-free; plain names (alphanumeric/`-`/`_`/`.`)
/// are left unchanged, so existing artifact filenames are unaffected.
#[cfg(feature = "host")]
fn sanitize_artifact_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            '\\' => out.push_str("%5C"),
            ':' => out.push_str("%3A"),
            _ => out.push(c),
        }
    }
    out
}

/// Maximum length of a single filename component on common filesystems.
#[cfg(feature = "host")]
const NAME_MAX: usize = 255;

/// The encoded filename length if it exceeds `NAME_MAX`, else `None`. A deeply
/// nested dependency store key can stay within the total path limit yet blow the
/// per-component limit once its separators are percent-encoded.
#[cfg(feature = "host")]
fn artifact_name_overflow(name: &str) -> Option<usize> {
    let len = sanitize_artifact_name(name).len();
    (len > NAME_MAX).then_some(len)
}

/// What follows a canister's encoded name in the name of the file that says
/// where its [linked](Access::link) module is.
///
/// No encoded name contains it, so the two kinds of file cannot be mistaken
/// for each other whatever canisters are called: [`sanitize_artifact_name`]
/// writes a `%` only ahead of one of four pairs of hex digits, and `%l` is
/// none of them.
#[cfg(feature = "host")]
const LINK_SUFFIX: &str = "%link";

/// As [`artifact_name_overflow`], for the file that records a link.
#[cfg(feature = "host")]
fn link_name_overflow(name: &str) -> Option<usize> {
    let len = sanitize_artifact_name(name).len() + LINK_SUFFIX.len();
    (len > NAME_MAX).then_some(len)
}

/// Where a [linked](Access::link) module is and what it hashed to, as the
/// store keeps it on disk.
#[cfg(feature = "host")]
#[derive(serde::Serialize, serde::Deserialize)]
struct Link {
    path: PathBuf,
    sha256: String,
}

#[cfg(feature = "host")]
impl ArtifactPaths {
    /// The module itself, for a canister whose artifact was saved.
    fn artifact_by_name(&self, name: &str) -> PathBuf {
        self.dir.join(sanitize_artifact_name(name))
    }

    /// The [`Link`], for a canister whose artifact was linked. A canister has
    /// one file or the other, never both.
    fn link_by_name(&self, name: &str) -> PathBuf {
        self.dir
            .join(format!("{}{LINK_SUFFIX}", sanitize_artifact_name(name)))
    }
}

/// Remove `path`, which need not be there.
#[cfg(feature = "host")]
fn remove_if_present(path: &Path) -> Result<(), crate::fs::IoError> {
    match remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(feature = "host")]
impl PathsAccess for ArtifactPaths {
    fn lock_file(&self) -> PathBuf {
        self.dir.join(".lock")
    }
}

#[cfg(feature = "host")]
impl ArtifactStore {
    pub fn new(project_root_locate: Arc<dyn ProjectRootLocate>) -> Self {
        Self {
            project_root_locate,
        }
    }

    /// Locked directory access for the artifact store. It will create the directory if it does not exist.
    fn lock(&self) -> Result<DirectoryStructureLock<ArtifactPaths>, crate::fs::lock::LockError> {
        let project_root = self
            .project_root_locate
            .locate()
            .expect("failed to locate project root");
        let artifact_dir = project_root
            .join(ICP_BASE)
            .join(CACHE_DIR)
            .join("artifacts");
        DirectoryStructureLock::open_or_create(ArtifactPaths { dir: artifact_dir })
    }
}

/// Carries what went wrong inside the store into [`SaveError::SaveStore`],
/// whole: the cause's own chain is what says which file it was and why it
/// failed, and this layer has nothing to add to it.
///
/// A free function rather than a closure because the store's several steps fail
/// in their own types — a lock, a write — and each is carried as itself.
#[cfg(feature = "host")]
fn save_store(name: &str, source: impl std::error::Error + Send + Sync + 'static) -> SaveError {
    SaveError::SaveStore {
        source: StoreCause::new(source),
        name: name.to_owned(),
    }
}

/// As [`save_store`], for [`LookupArtifactError::LookupStore`].
#[cfg(feature = "host")]
fn lookup_store(
    name: &str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> LookupArtifactError {
    LookupArtifactError::LookupStore {
        source: StoreCause::new(source),
        name: name.to_owned(),
    }
}

#[async_trait]
#[cfg(feature = "host")]
impl Access for ArtifactStore {
    async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        if let Some(len) = artifact_name_overflow(name) {
            return SaveNameTooLongSnafu {
                name: name.to_owned(),
                len,
            }
            .fail();
        }
        self.lock()
            .map_err(|e| save_store(name, e))?
            .with_write(async |store| {
                // The link goes first: with neither file the canister is
                // unbuilt, which is true, and with both it would be ambiguous.
                // A name too long to have a link has none to remove.
                if link_name_overflow(name).is_none() {
                    remove_if_present(&store.link_by_name(name))
                        .map_err(|e| save_store(name, e))?;
                }
                write(&store.artifact_by_name(name), wasm).map_err(|e| save_store(name, e))
            })
            .await
            .map_err(|e| save_store(name, e))?
    }

    async fn link(&self, name: &str, path: &Path, sha256: &str) -> Result<(), SaveError> {
        if let Some(len) = link_name_overflow(name) {
            return SaveNameTooLongSnafu {
                name: name.to_owned(),
                len,
            }
            .fail();
        }
        let link = Link {
            path: path.to_owned(),
            sha256: sha256.to_owned(),
        };
        self.lock()
            .map_err(|e| save_store(name, e))?
            .with_write(async |store| {
                // As in `save`: a module saved for this canister before is
                // gone before the link stands in for it.
                remove_if_present(&store.artifact_by_name(name))
                    .map_err(|e| save_store(name, e))?;
                json::save(&store.link_by_name(name), &link).map_err(|e| save_store(name, e))
            })
            .await
            .map_err(|e| save_store(name, e))?
    }

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupArtifactError> {
        if let Some(len) = artifact_name_overflow(name) {
            return LookupNameTooLongSnafu {
                name: name.to_owned(),
                len,
            }
            .fail();
        }
        self.lock()
            .map_err(|e| lookup_store(name, e))?
            .with_read(async |store| {
                // A linked module is read from where it lies, and handed over
                // only if it is still the one that was linked.
                let link = store.link_by_name(name);
                if link.exists() {
                    let Link { path, sha256 } =
                        json::load(&link).map_err(|e| lookup_store(name, e))?;
                    let wasm = read(&path).map_err(|e| LookupArtifactError::LookupLinkedRead {
                        source: StoreCause::new(e),
                        name: name.to_owned(),
                        path: path.clone(),
                    })?;
                    return verify_linked(name, &path, &sha256, wasm);
                }

                let artifact = store.artifact_by_name(name);
                // Not Found
                if !artifact.exists() {
                    return LookupArtifactNotFoundSnafu {
                        name: name.to_owned(),
                    }
                    .fail();
                }

                read(&artifact).map_err(|e| lookup_store(name, e))
            })
            .await
            .map_err(|e| lookup_store(name, e))?
    }
}

#[cfg(any(test, feature = "test-util"))]
/// In-memory mock implementation of `Access`.
///
/// A saved artifact is held in memory. A linked one is not held at all: only
/// where it is, and it is read from this machine's disk when it is looked up.
pub(crate) struct MockInMemoryArtifactStore {
    store: Mutex<HashMap<String, MockArtifact>>,
}

#[cfg(any(test, feature = "test-util"))]
enum MockArtifact {
    Saved(Vec<u8>),
    Linked { path: PathBuf, sha256: String },
}

#[cfg(any(test, feature = "test-util"))]
impl MockInMemoryArtifactStore {
    /// Creates a new empty in-memory artifact store.
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
impl Default for MockInMemoryArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl Access for MockInMemoryArtifactStore {
    async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        let mut store = self.store.lock().unwrap();
        store.insert(name.to_string(), MockArtifact::Saved(wasm.to_vec()));
        Ok(())
    }

    async fn link(&self, name: &str, path: &Path, sha256: &str) -> Result<(), SaveError> {
        let mut store = self.store.lock().unwrap();
        store.insert(
            name.to_string(),
            MockArtifact::Linked {
                path: path.to_owned(),
                sha256: sha256.to_owned(),
            },
        );
        Ok(())
    }

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupArtifactError> {
        let store = self.store.lock().unwrap();

        match store.get(name) {
            Some(MockArtifact::Saved(wasm)) => Ok(wasm.clone()),
            Some(MockArtifact::Linked { path, sha256 }) => {
                let wasm =
                    std::fs::read(path).map_err(|e| LookupArtifactError::LookupLinkedRead {
                        source: StoreCause::new(e),
                        name: name.to_owned(),
                        path: path.clone(),
                    })?;
                verify_linked(name, path, sha256, wasm)
            }
            None => Err(LookupArtifactError::LookupArtifactNotFound {
                name: name.to_owned(),
            }),
        }
    }
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use camino_tempfile::{Utf8TempDir, tempdir};

    use super::*;
    use crate::manifest::ProjectRootLocateError;

    /// A project at a directory, which is all the store asks to be told.
    struct At(PathBuf);

    impl ProjectRootLocate for At {
        fn locate(&self) -> Result<PathBuf, ProjectRootLocateError> {
            Ok(self.0.clone())
        }
        fn locate_member(&self) -> Result<PathBuf, ProjectRootLocateError> {
            Ok(self.0.clone())
        }
    }

    const MODULE: &[u8] = b"\0asm a module, and long enough not to be mistaken for a path";

    /// A project directory, the store in it, and a module beside it: outside
    /// the project, as a cached download is.
    fn store() -> (Utf8TempDir, ArtifactStore, PathBuf) {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let module = dir.path().join("module.wasm");
        std::fs::write(&module, MODULE).unwrap();
        (dir, ArtifactStore::new(Arc::new(At(project))), module)
    }

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    /// Every file under `dir`, with what it holds.
    fn files_under(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in crate::fs::read_dir(dir).unwrap() {
            if entry.is_dir() {
                out.extend(files_under(&entry));
            } else {
                let contents = std::fs::read(&entry).unwrap();
                out.push((entry, contents));
            }
        }
        out
    }

    /// The point of a link: the module is looked up from where it lies, and
    /// the project holds no copy of it, however many canisters name it.
    #[tokio::test]
    async fn a_linked_module_is_looked_up_without_a_copy_of_it_in_the_project() {
        let (dir, store, module) = store();

        for name in ["frontend", "admin", "vendor/lib:frontend"] {
            store.link(name, &module, &sha256(MODULE)).await.unwrap();
        }

        for name in ["frontend", "admin", "vendor/lib:frontend"] {
            assert_eq!(store.lookup(name).await.unwrap(), MODULE);
        }
        let kept = files_under(&dir.path().join("project"));
        assert!(!kept.is_empty(), "the store records each link");
        for (file, contents) in kept {
            assert!(
                !contents
                    .windows(MODULE.len())
                    .any(|window| window == MODULE),
                "'{file}' holds the module"
            );
        }
        assert_eq!(std::fs::read(&module).unwrap(), MODULE);
    }

    /// The file is its owner's and can change under the link. What was built
    /// is then not there to hand out, and the lookup says so, naming the file.
    #[tokio::test]
    async fn a_linked_module_that_changed_is_refused() {
        let (_dir, store, module) = store();
        store
            .link("backend", &module, &sha256(MODULE))
            .await
            .unwrap();

        std::fs::write(&module, b"another module").unwrap();

        let err = store.lookup("backend").await.unwrap_err();
        assert!(
            matches!(
                &err,
                LookupArtifactError::LookupLinkedChanged { path, expected, actual, .. }
                    if *path == module
                        && *expected == sha256(MODULE)
                        && *actual == sha256(b"another module")
            ),
            "{err:?}"
        );
        let text = err.to_string();
        assert!(text.contains(module.as_str()), "{text}");
        assert!(text.contains("build the canister again"), "{text}");
    }

    #[tokio::test]
    async fn a_linked_module_that_is_gone_is_refused() {
        let (_dir, store, module) = store();
        store
            .link("backend", &module, &sha256(MODULE))
            .await
            .unwrap();

        std::fs::remove_file(&module).unwrap();

        let err = store.lookup("backend").await.unwrap_err();
        assert!(
            matches!(
                &err,
                LookupArtifactError::LookupLinkedRead { path, .. } if *path == module
            ),
            "{err:?}"
        );
        let text = err.to_string();
        assert!(text.contains(module.as_str()), "{text}");
        assert!(text.contains("build the canister again"), "{text}");
    }

    /// A canister has the artifact of its last build, whichever kind that was:
    /// neither a link nor a saved module outlives the build that replaced it.
    #[tokio::test]
    async fn the_last_build_of_a_canister_is_the_one_looked_up() {
        let (dir, store, module) = store();
        let artifacts = dir.path().join("project/.icp/cache/artifacts");

        store.save("backend", b"saved").await.unwrap();
        store
            .link("backend", &module, &sha256(MODULE))
            .await
            .unwrap();
        assert_eq!(store.lookup("backend").await.unwrap(), MODULE);
        assert!(
            !artifacts.join("backend").exists(),
            "the module saved before is left behind"
        );

        store.save("backend", b"saved again").await.unwrap();
        // The link would win the lookup if it were still there, and would fail
        // it: the file it names no longer hashes to what it was linked with.
        std::fs::write(&module, b"another module").unwrap();
        assert_eq!(store.lookup("backend").await.unwrap(), b"saved again");
    }

    /// A link is not a module under another canister's name: `backend` linked
    /// leaves nothing to be read as the artifact of a canister whose name is
    /// `backend` and then some.
    #[tokio::test]
    async fn a_link_is_no_other_canisters_artifact() {
        let (dir, store, module) = store();
        store
            .link("backend", &module, &sha256(MODULE))
            .await
            .unwrap();

        let artifacts = dir.path().join("project/.icp/cache/artifacts");
        for file in crate::fs::read_dir(&artifacts).unwrap() {
            let recorded_as = file.file_name().unwrap();
            if recorded_as == ".lock" {
                continue;
            }
            // Whatever the link's file is called, no name encodes to it.
            for name in ["backend", "backend%link", "backend.link", recorded_as] {
                assert_ne!(
                    sanitize_artifact_name(name),
                    recorded_as,
                    "'{name}' is stored where the link is"
                );
            }
        }
    }

    /// A link's file is named longer than a module's, so a name that only just
    /// fits a module is refused a link, before anything is written.
    #[tokio::test]
    async fn a_name_with_no_room_for_a_link_is_refused_one() {
        let (dir, store, module) = store();
        let fits_a_module = "a".repeat(NAME_MAX);
        assert!(artifact_name_overflow(&fits_a_module).is_none());

        let err = store
            .link(&fits_a_module, &module, &sha256(MODULE))
            .await
            .unwrap_err();

        assert!(
            matches!(err, SaveError::SaveNameTooLong { len, .. } if len > NAME_MAX),
            "{err:?}"
        );
        assert!(!dir.path().join("project/.icp").exists());

        // And it is still a name a module can be saved and looked up under.
        store.save(&fits_a_module, b"saved").await.unwrap();
        assert_eq!(store.lookup(&fits_a_module).await.unwrap(), b"saved");
    }

    #[test]
    fn plain_names_unchanged() {
        assert_eq!(sanitize_artifact_name("backend"), "backend");
        assert_eq!(
            sanitize_artifact_name("my-canister_1.wasm"),
            "my-canister_1.wasm"
        );
    }

    #[test]
    fn namespaced_names_are_filename_safe() {
        let s = sanitize_artifact_name("vendor/openemail:backend");
        assert_eq!(s, "vendor%2Fopenemail%3Abackend");
        assert!(!s.contains('/'));
        assert!(!s.contains(':'));
    }

    #[test]
    fn encoding_is_injective() {
        // `%` is itself encoded, so a literal "%2F" never collides with "/".
        assert_ne!(
            sanitize_artifact_name("a%2Fb"),
            sanitize_artifact_name("a/b")
        );
    }

    #[test]
    fn artifact_name_overflow_flags_only_over_limit_names() {
        assert!(artifact_name_overflow("backend").is_none());
        assert!(artifact_name_overflow("vendor/openemail:backend").is_none());
        // A pathologically deep store key stays within the total path limit but
        // its single encoded segment exceeds NAME_MAX once separators are encoded.
        let deep = format!("{}:leaf", vec!["dir"; 80].join("/"));
        assert!(artifact_name_overflow(&deep).is_some());
    }
}
