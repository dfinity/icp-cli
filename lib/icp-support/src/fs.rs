use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("failed to read {path}")]
pub struct ReadFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn read(path: &Path) -> Result<Vec<u8>, ReadFileError> {
    std::fs::read(path).map_err(|source| ReadFileError {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Error, Debug)]
#[error("failed to remove file {path}")]
pub struct RemoveFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn remove_file(path: &Path) -> Result<(), RemoveFileError> {
    std::fs::remove_file(path).map_err(|source| RemoveFileError {
        path: path.to_path_buf(),
        source,
    })
}
