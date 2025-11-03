#[cfg(test)]
use std::{collections::HashMap, sync::Mutex};

use crate::{
    fs::{
        lock::{DirectoryStructureLock, PathsAccess},
        read, write,
    },
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
    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError>;
}

#[derive(Debug, Snafu)]
pub enum SaveError {
    #[snafu(display("failed to write artifact file"))]
    SaveWriteFileError { source: crate::fs::Error },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(display("failed to read artifact file"))]
    LookupReadFileError { source: crate::fs::Error },

    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },
}

pub(crate) struct ArtifactStore {
    lock: DirectoryStructureLock<ArtifactPaths>,
}

struct ArtifactPaths {
    dir: PathBuf,
}

impl ArtifactPaths {
    fn artifact_by_name(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl PathsAccess for ArtifactPaths {
    fn lock_file(&self) -> PathBuf {
        self.dir.join(".lock")
    }
}

impl ArtifactStore {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            lock: DirectoryStructureLock::open_or_create(ArtifactPaths {
                dir: path.to_owned(),
            })
            .expect("failed to create artifact store lock"),
        }
    }
}

#[async_trait]
impl Access for ArtifactStore {
    async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        self.lock
            .with_write(async |store| {
                // Save artifact
                write(&store.artifact_by_name(name), wasm).context(SaveWriteFileSnafu)?;
                Ok(())
            })
            .await?
    }

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError> {
        self.lock
            .with_read(async |store| {
                let artifact = store.artifact_by_name(name);
                // Not Found
                if !artifact.exists() {
                    return Err(LookupError::LookupArtifactNotFound {
                        name: name.to_owned(),
                    });
                }

                // Load artifact
                let wasm = read(&artifact).context(LookupReadFileSnafu)?;

                Ok(wasm)
            })
            .await?
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

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError> {
        let store = self.store.lock().unwrap();

        match store.get(name) {
            Some(wasm) => Ok(wasm.clone()),
            None => Err(LookupError::LookupArtifactNotFound {
                name: name.to_owned(),
            }),
        }
    }
}
