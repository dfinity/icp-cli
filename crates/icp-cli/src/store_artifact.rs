use icp::{
    fs::{create_dir_all, read, write},
    prelude::*,
};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum SaveError {
    #[snafu(display("failed to create artifacts directory"))]
    ArtifactsDir { source: icp::fs::Error },

    #[snafu(display("failed to write artifact file"))]
    SaveWriteFileError { source: icp::fs::Error },
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(display("failed to read artifact file"))]
    LookupReadFileError { source: icp::fs::Error },

    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },
}

pub struct ArtifactStore(PathBuf);

impl ArtifactStore {
    pub fn new(path: &PathBuf) -> Self {
        Self(path.clone())
    }
}

impl ArtifactStore {
    pub fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        // Create artifacts directory
        create_dir_all(&self.0).context(ArtifactsDirSnafu)?;

        // Store artifact
        write(&self.0.join(name), wasm).context(SaveWriteFileSnafu)?;

        Ok(())
    }

    pub fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError> {
        // Not Found
        if !self.0.join(name).exists() {
            return Err(LookupError::LookupArtifactNotFound {
                name: name.to_owned(),
            });
        }

        // Load artifact
        let wasm = read(&self.0.join(name)).context(LookupReadFileSnafu)?;

        Ok(wasm)
    }
}
