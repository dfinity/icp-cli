use std::sync::Arc;
#[cfg(test)]
use std::{collections::HashMap, sync::Mutex};

use crate::{
    CACHE_DIR, ICP_BASE,
    fs::{
        lock::{DirectoryStructureLock, PathsAccess},
        read, write,
    },
    manifest::ProjectRootLocate,
    prelude::*,
};
use async_trait::async_trait;
use snafu::{ResultExt, Snafu};

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
    #[snafu(display("failed to write artifact file"))]
    SaveWriteFileError { source: crate::fs::IoError },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },
}

#[derive(Debug, Snafu)]
pub enum LookupArtifactError {
    #[snafu(display("failed to read artifact file"))]
    LookupReadFileError { source: crate::fs::IoError },

    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },
}

pub(crate) struct ArtifactStore {
    project_root_locate: Arc<dyn ProjectRootLocate>,
}

struct ArtifactPaths {
    dir: PathBuf,
}

/// Encode a canister name into a single filename-safe segment.
///
/// Canister names may be namespaced store keys containing `/` and `:` (imported
/// dependency canisters, e.g. `vendor/openemail:backend`), which are not valid
/// filename characters on every platform. Percent-encoding the unsafe set keeps
/// the mapping reversible and collision-free; plain names (alphanumeric/`-`/`_`/`.`)
/// are left unchanged, so existing artifact filenames are unaffected.
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

impl ArtifactPaths {
    fn artifact_by_name(&self, name: &str) -> PathBuf {
        self.dir.join(sanitize_artifact_name(name))
    }
}

impl PathsAccess for ArtifactPaths {
    fn lock_file(&self) -> PathBuf {
        self.dir.join(".lock")
    }
}

impl ArtifactStore {
    pub(crate) fn new(project_root_locate: Arc<dyn ProjectRootLocate>) -> Self {
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

#[async_trait]
impl Access for ArtifactStore {
    async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        self.lock()?
            .with_write(async |store| {
                // Save artifact
                write(&store.artifact_by_name(name), wasm).context(SaveWriteFileSnafu)?;
                Ok(())
            })
            .await?
    }

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupArtifactError> {
        self.lock()?
            .with_read(async |store| {
                let artifact = store.artifact_by_name(name);
                // Not Found
                if !artifact.exists() {
                    return LookupArtifactNotFoundSnafu {
                        name: name.to_owned(),
                    }
                    .fail();
                }

                // Load artifact
                let wasm = read(&artifact).context(LookupReadFileSnafu)?;

                Ok(wasm)
            })
            .await?
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_artifact_name;

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
}

#[cfg(test)]
/// In-memory mock implementation of `Access`.
pub(crate) struct MockInMemoryArtifactStore {
    store: Mutex<HashMap<String, Vec<u8>>>,
}

#[cfg(test)]
impl MockInMemoryArtifactStore {
    /// Creates a new empty in-memory artifact store.
    pub(crate) fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
impl Default for MockInMemoryArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
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
