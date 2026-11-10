#[cfg(feature = "host")]
use std::sync::Arc;
#[cfg(any(test, feature = "test-util"))]
use std::{collections::HashMap, sync::Mutex};

#[cfg(feature = "host")]
use crate::{
    CACHE_DIR, ICP_BASE,
    files::{Confined, FsError},
    fs::{
        lock::{DirectoryStructureLock, PathsAccess},
        read, write,
    },
    manifest::ProjectRootLocate,
    prelude::*,
    store_id::StoreCause,
};
use async_trait::async_trait;
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

#[cfg(feature = "host")]
pub struct ArtifactStore {
    project_root_locate: Arc<dyn ProjectRootLocate>,
    confined: Option<Confined>,
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
    fn artifact_by_name(&self, name: &str) -> PathBuf {
        self.dir.join(sanitize_artifact_name(name))
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
    /// `confined` is given for a project that is not trusted: the store's
    /// files are then held to it, so a symlink planted where the store keeps
    /// one is refused rather than written or read through.
    pub fn new(
        project_root_locate: Arc<dyn ProjectRootLocate>,
        confined: Option<Confined>,
    ) -> Self {
        Self {
            project_root_locate,
            confined,
        }
    }

    fn paths(&self) -> ArtifactPaths {
        let project_root = self
            .project_root_locate
            .locate()
            .expect("failed to locate project root");
        ArtifactPaths {
            dir: project_root
                .join(ICP_BASE)
                .join(CACHE_DIR)
                .join("artifacts"),
        }
    }

    /// Refuse the lock file and `name`'s artifact unless the filesystem the
    /// project is confined to, if it is, would give them.
    fn held(&self, name: &str) -> Result<(), FsError> {
        if let Some(confined) = &self.confined {
            let paths = self.paths();
            confined.resolve(&paths.lock_file())?;
            confined.resolve(&paths.artifact_by_name(name))?;
        }
        Ok(())
    }

    /// Locked directory access for the artifact store. It will create the directory if it does not exist.
    fn lock(&self) -> Result<DirectoryStructureLock<ArtifactPaths>, crate::fs::lock::LockError> {
        DirectoryStructureLock::open_or_create(self.paths())
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
        self.held(name).map_err(|e| save_store(name, e))?;
        self.lock()
            .map_err(|e| save_store(name, e))?
            .with_write(async |store| {
                write(&store.artifact_by_name(name), wasm).map_err(|e| save_store(name, e))
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
        self.held(name).map_err(|e| lookup_store(name, e))?;
        self.lock()
            .map_err(|e| lookup_store(name, e))?
            .with_read(async |store| {
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
    use super::{artifact_name_overflow, sanitize_artifact_name};

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

#[cfg(all(test, unix, feature = "host"))]
mod confined_tests {
    use super::*;
    use crate::manifest::ProjectRootLocateImpl;

    /// A link planted where the store keeps an artifact, or its lock, is
    /// refused when the project is confined, rather than written through.
    #[tokio::test]
    async fn an_artifact_behind_a_symlink_is_refused_when_confined() {
        let dir = camino_tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let artifacts = root.join(ICP_BASE).join(CACHE_DIR).join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(root.join("icp.yaml"), "canisters: []").unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "mine").unwrap();
        let locate = Arc::new(ProjectRootLocateImpl::new(root.clone(), Some(root.clone())));
        let store = ArtifactStore::new(locate, Some(Confined::new(&root).unwrap()));

        for planted in ["backend", ".lock"] {
            let link = artifacts.join(planted);
            std::os::unix::fs::symlink(&victim, &link).unwrap();
            let err = store.save("backend", b"wasm").await.unwrap_err();
            assert!(
                matches!(err, SaveError::SaveStore { .. }),
                "{planted}: {err}"
            );
            let err = store.lookup("backend").await.unwrap_err();
            assert!(
                matches!(err, LookupArtifactError::LookupStore { .. }),
                "{planted}: {err}"
            );
            assert_eq!(
                std::fs::read_to_string(&victim).unwrap(),
                "mine",
                "{planted}"
            );
            std::fs::remove_file(&link).unwrap();
        }

        store
            .save("backend", b"wasm")
            .await
            .expect("no link, saved");
        assert_eq!(store.lookup("backend").await.unwrap(), b"wasm");
    }
}
