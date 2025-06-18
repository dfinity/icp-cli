use camino::Utf8PathBuf;
use icp_fs::fs::{ReadFileError, WriteFileError, read, write};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum RegisterError {
    #[snafu(display("failed to write artifact file"))]
    RegisterWriteFileError { source: WriteFileError },
}

pub trait Register {
    fn register(&self, name: &str, wasm: &[u8]) -> Result<(), RegisterError>;
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(display("failed to read artifact file"))]
    LookupReadFileError { source: ReadFileError },

    #[snafu(display("could not find artifact for canister '{name}'"))]
    LookupArtifactNotFound { name: String },
}

pub trait Lookup {
    fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError>;
}

pub struct ArtifactStore(Utf8PathBuf);

impl ArtifactStore {
    pub fn new(path: &Utf8PathBuf) -> Self {
        Self(path.clone())
    }
}

impl Register for ArtifactStore {
    fn register(&self, name: &str, wasm: &[u8]) -> Result<(), RegisterError> {
        // Store artifact
        write(self.0.join(name), wasm).context(RegisterWriteFileSnafu)?;

        Ok(())
    }
}

impl Lookup for ArtifactStore {
    fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupError> {
        // Not Found
        if !self.0.join(name).exists() {
            return Err(LookupError::LookupArtifactNotFound {
                name: name.to_owned(),
            });
        }

        // Load artifact
        let wasm = read(self.0.join(name)).context(LookupReadFileSnafu)?;

        Ok(wasm)
    }
}
