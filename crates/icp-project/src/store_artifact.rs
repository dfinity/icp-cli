#[cfg(feature = "host")]
use std::sync::Arc;
#[cfg(any(test, feature = "test-util"))]
use std::{collections::HashMap, sync::Mutex};

#[cfg(feature = "host")]
use crate::{
    CACHE_DIR, ICP_BASE,
    fs::{
        create_dir_all,
        lock::{DirectoryStructureLock, PathsAccess},
        read, read_dir, remove_file, rename, write, write_string,
    },
    manifest::ProjectRootLocate,
    prelude::*,
    store_id::StoreCause,
};
use async_trait::async_trait;
#[cfg(feature = "host")]
use sha2::{Digest, Sha256};
use snafu::Snafu;

#[async_trait]
/// Trait for accessing and managing canister build artifacts.
pub trait Access: Sync + Send {
    /// Save a canister artifact (WASM) to the store.
    async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError>;

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
}

/// The project's build artifacts, under `.icp/cache/artifacts`.
///
/// Each distinct module is kept once, named by its SHA-256, and a canister's
/// entry is a small file naming the module it was last built to:
///
/// ```text
/// artifacts/
/// ├── .modules/<sha256>.wasm    one per distinct module
/// └── .refs/<canister>          the SHA-256 of that canister's module
/// ```
///
/// Canisters built to the same bytes — several frontends on one asset canister,
/// say — share the one file, and a rebuild that changes nothing writes no
/// module. A module is removed when the last canister naming it is rebuilt to
/// something else.
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

#[cfg(feature = "host")]
impl ArtifactPaths {
    // Both directories are named with a leading `.`, which no canister name
    // may contain: a store from before modules were shared kept each module in
    // a file named for its canister, directly in the store, and neither
    // directory can be where one of those files is.
    fn modules_dir(&self) -> PathBuf {
        self.dir.join(".modules")
    }

    fn refs_dir(&self) -> PathBuf {
        self.dir.join(".refs")
    }

    /// Where the module with this SHA-256 is kept.
    fn module_by_sha256(&self, sha256: &str) -> PathBuf {
        self.modules_dir().join(format!("{sha256}.wasm"))
    }

    /// The file naming the module a canister was last built to.
    fn ref_by_name(&self, name: &str) -> PathBuf {
        self.refs_dir().join(sanitize_artifact_name(name))
    }

    /// Record `wasm` as the module `name` was built to.
    fn save(&self, name: &str, wasm: &[u8]) -> Result<(), StoreCause> {
        let sha256 = hex::encode(Sha256::digest(wasm));

        create_dir_all(&self.modules_dir()).map_err(StoreCause::new)?;
        create_dir_all(&self.refs_dir()).map_err(StoreCause::new)?;

        let reference = self.ref_by_name(name);
        let previous = read_ref(&reference)?;

        let module = self.module_by_sha256(&sha256);
        if !module.is_file() {
            // Written beside its final name and moved into place, so a module
            // is never there half-written: that it exists is all that is
            // checked before the write is skipped.
            let partial = module.with_extension("partial");
            write(&partial, wasm).map_err(StoreCause::new)?;
            rename(&partial, &module).map_err(StoreCause::new)?;
        }
        write_string(&reference, &sha256).map_err(StoreCause::new)?;

        // Only this save can have orphaned the module it replaced.
        if let Some(previous) = previous.filter(|previous| *previous != sha256)
            && !self.is_referenced(&previous)?
        {
            let orphan = self.module_by_sha256(&previous);
            if orphan.is_file() {
                remove_file(&orphan).map_err(StoreCause::new)?;
            }
        }

        Ok(())
    }

    /// The module `name` was last built to, if the store has one.
    fn lookup(&self, name: &str) -> Result<Option<Vec<u8>>, StoreCause> {
        let Some(sha256) = read_ref(&self.ref_by_name(name))? else {
            return Ok(None);
        };
        let module = self.module_by_sha256(&sha256);
        if !module.is_file() {
            return Ok(None);
        }
        read(&module).map(Some).map_err(StoreCause::new)
    }

    /// Whether any canister's entry names this module.
    fn is_referenced(&self, sha256: &str) -> Result<bool, StoreCause> {
        for reference in read_dir(&self.refs_dir()).map_err(StoreCause::new)? {
            if read_ref(&reference)?.as_deref() == Some(sha256) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// The SHA-256 a canister's entry names, or `None` when there is no entry or it
/// does not hold one — which is read as nothing having been built, since a
/// rebuild is what puts it right.
#[cfg(feature = "host")]
fn read_ref(reference: &Path) -> Result<Option<String>, StoreCause> {
    if !reference.is_file() {
        return Ok(None);
    }
    // Read as bytes: an entry that is not text is one that does not hold a
    // SHA-256, not a failure to read it.
    let contents = read(reference).map_err(StoreCause::new)?;
    let Ok(contents) = std::str::from_utf8(&contents) else {
        return Ok(None);
    };
    let sha256 = contents.trim();
    let is_sha256 = sha256.len() == 64
        && sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    Ok(is_sha256.then(|| sha256.to_owned()))
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
            .with_write(async |store| store.save(name, wasm))
            .await
            .map_err(|e| save_store(name, e))?
            .map_err(|source| SaveError::SaveStore {
                source,
                name: name.to_owned(),
            })
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
            .with_read(async |store| store.lookup(name))
            .await
            .map_err(|e| lookup_store(name, e))?
            .map_err(|source| LookupArtifactError::LookupStore {
                source,
                name: name.to_owned(),
            })?
            .ok_or_else(|| {
                LookupArtifactNotFoundSnafu {
                    name: name.to_owned(),
                }
                .build()
            })
    }
}

#[cfg(any(test, feature = "test-util"))]
/// In-memory mock implementation of `Access`.
pub(crate) struct MockInMemoryArtifactStore {
    store: Mutex<HashMap<String, Vec<u8>>>,
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
        store.insert(name.to_string(), wasm.to_vec());
        Ok(())
    }

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupArtifactError> {
        let store = self.store.lock().unwrap();

        match store.get(name) {
            Some(wasm) => Ok(wasm.clone()),
            None => Err(LookupArtifactError::LookupArtifactNotFound {
                name: name.to_owned(),
            }),
        }
    }
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use std::sync::Arc;

    use camino_tempfile::Utf8TempDir;

    use super::{
        Access, ArtifactStore, LookupArtifactError, artifact_name_overflow, sanitize_artifact_name,
    };
    use crate::{
        fs::{read_dir, read_to_string, write},
        manifest::{ProjectRootLocate, ProjectRootLocateError},
        prelude::*,
    };

    struct Root(PathBuf);

    impl ProjectRootLocate for Root {
        fn locate(&self) -> Result<PathBuf, ProjectRootLocateError> {
            Ok(self.0.clone())
        }

        fn locate_member(&self) -> Result<PathBuf, ProjectRootLocateError> {
            Ok(self.0.clone())
        }
    }

    /// A store in a project of its own, and the directory it keeps artifacts in.
    fn store() -> (Utf8TempDir, ArtifactStore, PathBuf) {
        let project = Utf8TempDir::new().unwrap();
        let store = ArtifactStore::new(Arc::new(Root(project.path().to_owned())));
        let dir = project.path().join(".icp/cache/artifacts");
        (project, store, dir)
    }

    async fn assert_not_built(store: &ArtifactStore, name: &str) {
        assert!(matches!(
            store.lookup(name).await,
            Err(LookupArtifactError::LookupArtifactNotFound { .. })
        ));
    }

    /// The file names under the store's `.modules` directory.
    fn modules(dir: &Path) -> Vec<String> {
        read_dir(&dir.join(".modules"))
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_owned())
            .collect()
    }

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[tokio::test]
    async fn a_canister_never_built_is_not_found() {
        let (_project, store, _) = store();
        store.save("backend", b"module").await.unwrap();
        assert_not_built(&store, "frontend").await;
    }

    #[tokio::test]
    async fn canisters_built_to_the_same_bytes_share_one_module() {
        let (_project, store, dir) = store();
        store.save("first", b"module").await.unwrap();
        store.save("second", b"module").await.unwrap();
        store.save("vendor/dep:third", b"module").await.unwrap();
        // A rebuild that changes nothing.
        store.save("first", b"module").await.unwrap();

        assert_eq!(modules(&dir).len(), 1);
        for name in ["first", "second", "vendor/dep:third"] {
            assert_eq!(store.lookup(name).await.unwrap(), b"module");
        }
    }

    #[tokio::test]
    async fn a_module_is_named_by_its_sha256() {
        let (_project, store, dir) = store();
        store.save("backend", b"").await.unwrap();

        assert_eq!(modules(&dir), [format!("{EMPTY_SHA256}.wasm")]);
        assert_eq!(
            read_to_string(&dir.join(".refs/backend")).unwrap(),
            EMPTY_SHA256
        );
    }

    #[tokio::test]
    async fn a_replaced_module_is_removed_once_nothing_names_it() {
        let (_project, store, dir) = store();
        store.save("first", b"old").await.unwrap();
        store.save("second", b"old").await.unwrap();

        // `second` still names the old module.
        store.save("first", b"new").await.unwrap();
        assert_eq!(modules(&dir).len(), 2);
        assert_eq!(store.lookup("second").await.unwrap(), b"old");

        store.save("second", b"new").await.unwrap();
        assert_eq!(modules(&dir).len(), 1);
        assert_eq!(store.lookup("first").await.unwrap(), b"new");
        assert_eq!(store.lookup("second").await.unwrap(), b"new");
    }

    #[tokio::test]
    async fn an_entry_whose_module_is_gone_is_not_found() {
        let (_project, store, dir) = store();
        store.save("backend", b"").await.unwrap();
        std::fs::remove_file(dir.join(format!(".modules/{EMPTY_SHA256}.wasm"))).unwrap();

        assert_not_built(&store, "backend").await;
    }

    #[tokio::test]
    async fn an_entry_that_is_not_a_sha256_is_not_found_and_a_rebuild_replaces_it() {
        for entry in [&b"../../../somewhere"[..], &[0xff, 0xfe, 0x00]] {
            let (_project, store, dir) = store();
            store.save("backend", b"module").await.unwrap();
            store.save("other", b"old").await.unwrap();
            write(&dir.join(".refs/backend"), entry).unwrap();

            assert_not_built(&store, "backend").await;

            // Neither its own rebuild nor another canister's is held up by it.
            store.save("other", b"new").await.unwrap();
            store.save("backend", b"module").await.unwrap();
            assert_eq!(store.lookup("backend").await.unwrap(), b"module");
        }
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
