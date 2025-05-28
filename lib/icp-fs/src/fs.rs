use snafu::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Snafu, Debug)]
#[snafu(display("failed to create directory {} and parents", path.display()))]
pub struct CreateDirAllError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn create_dir_all(path: &Path) -> Result<(), CreateDirAllError> {
    std::fs::create_dir_all(path).context(CreateDirAllSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to read {}", path.display()))]
pub struct ReadFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn read<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, ReadFileError> {
    let path = path.as_ref();
    std::fs::read(path).context(ReadFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove directory {} and contents", path.display()))]
pub struct RemoveDirAllError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn remove_dir_all(path: &Path) -> Result<(), RemoveDirAllError> {
    std::fs::remove_dir_all(path).context(RemoveDirAllSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove file {}", path.display()))]
pub struct RemoveFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> Result<(), RemoveFileError> {
    let path = path.as_ref();
    std::fs::remove_file(path).context(RemoveFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to write {}", path.display()))]
pub struct WriteFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<(), WriteFileError> {
    let path = path.as_ref();
    std::fs::write(path, contents).context(WriteFileSnafu { path })
}
