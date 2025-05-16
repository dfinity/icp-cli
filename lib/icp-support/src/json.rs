use crate::fs::{ReadFileError, read};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoadJsonFileError {
    #[error("failed to parse {path} as json")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error(transparent)]
    Read(#[from] ReadFileError),
}

pub fn load_json_file<T: for<'a> serde::de::Deserialize<'a>>(
    path: &Path,
) -> Result<T, LoadJsonFileError> {
    let content = read(path)?;

    serde_json::from_slice(content.as_ref()).map_err(|source| LoadJsonFileError::Parse {
        path: path.to_path_buf(),
        source,
    })
}
