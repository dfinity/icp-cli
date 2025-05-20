use snafu::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Snafu, Debug)]
#[snafu(display("failed to read {}", path.display()))]
pub struct ReadFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn read(path: &Path) -> Result<Vec<u8>, ReadFileError> {
    std::fs::read(path).context(ReadFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove file {}", path.display()))]
pub struct RemoveFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn remove_file(path: &Path) -> Result<(), RemoveFileError> {
    std::fs::remove_file(path).context(RemoveFileSnafu { path })
}
