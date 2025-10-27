use std::sync::Mutex;

use icp::{
    fs::{create_dir_all, read, write},
    prelude::*,
};
use snafu::{ResultExt, Snafu};

/// Trait for accessing and managing canister build artifacts.
pub(crate) trait Access: Sync + Send {
    /// Save a canister artifact (WASM) to the store.
    fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError>;

    /// Lookup a canister artifact (WASM) from the store.
    fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError>;
}

#[derive(Debug, Snafu)]
pub(crate) enum SaveError {
    #[snafu(display("failed to create artifacts directory"))]
    ArtifactsDir { source: icp::fs::Error },

    #[snafu(display("failed to write artifact file"))]
    SaveWriteFileError { source: icp::fs::Error },
}

#[derive(Debug, Snafu)]
pub(crate) enum LookupError {
    #[snafu(display("failed to read artifact file"))]
    LookupReadFileError { source: icp::fs::Error },

    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },
}

pub(crate) struct ArtifactStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ArtifactStore {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            lock: Mutex::new(()),
        }
    }
}

impl Access for ArtifactStore {
    fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        // Lock Artifact Store
        let _g = self
            .lock
            .lock()
            .expect("failed to acquire artifact store lock");

        // Create artifacts directory
        create_dir_all(&self.path).context(ArtifactsDirSnafu)?;

        // Store artifact
        write(&self.path.join(name), wasm).context(SaveWriteFileSnafu)?;

        Ok(())
    }

    fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError> {
        // Lock Artifact Store
        let _g = self
            .lock
            .lock()
            .expect("failed to acquire artifact store lock");

        // Not Found
        if !self.path.join(name).exists() {
            return Err(LookupError::LookupArtifactNotFound {
                name: name.to_owned(),
            });
        }

        // Load artifact
        let wasm = read(&self.path.join(name)).context(LookupReadFileSnafu)?;

        Ok(wasm)
    }
}
