use icp::{
    fs::{
        lock::{DirectoryStructureLock, PathsAccess},
        read, write,
    },
    prelude::*,
};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub(crate) enum SaveError {
    #[snafu(display("failed to write artifact file"))]
    SaveWriteFileError { source: icp::fs::Error },

    #[snafu(transparent)]
    LockError { source: icp::fs::lock::LockError },
}

#[derive(Debug, Snafu)]
pub(crate) enum LookupError {
    #[snafu(display("failed to read artifact file"))]
    LookupReadFileError { source: icp::fs::Error },

    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },

    #[snafu(transparent)]
    LockError { source: icp::fs::lock::LockError },
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

impl ArtifactStore {
    pub(crate) async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
        self.lock
            .with_write(async |store| {
                // Save artifact
                write(&store.artifact_by_name(name), wasm).context(SaveWriteFileSnafu)?;
                Ok(())
            })
            .await?
    }

    pub(crate) async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError> {
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
