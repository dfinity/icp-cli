use camino::{Utf8Path, Utf8PathBuf};
use snafu::prelude::*;

#[derive(Snafu, Debug)]
#[snafu(display("failed to create directory {path} and parents"))]
pub struct CreateDirAllError {
    pub path: Utf8PathBuf,
    pub source: std::io::Error,
}

pub fn create_dir_all(path: impl AsRef<Utf8Path>) -> Result<(), CreateDirAllError> {
    let path = path.as_ref();
    std::fs::create_dir_all(path).context(CreateDirAllSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to read {path}"))]
pub struct ReadFileError {
    pub path: Utf8PathBuf,
    pub source: std::io::Error,
}

pub fn read<P: AsRef<Utf8Path>>(path: P) -> Result<Vec<u8>, ReadFileError> {
    let path = path.as_ref();
    std::fs::read(path).context(ReadFileSnafu { path })
}

pub fn read_to_string<P: AsRef<Utf8Path>>(path: P) -> Result<String, ReadFileError> {
    let path = path.as_ref();
    std::fs::read_to_string(path).context(ReadFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove directory {path} and contents"))]
pub struct RemoveDirAllError {
    pub path: Utf8PathBuf,
    pub source: std::io::Error,
}

pub fn remove_dir_all(path: impl AsRef<Utf8Path>) -> Result<(), RemoveDirAllError> {
    let path = path.as_ref();
    std::fs::remove_dir_all(path).context(RemoveDirAllSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove file {path}"))]
pub struct RemoveFileError {
    pub path: Utf8PathBuf,
    pub source: std::io::Error,
}

pub fn remove_file<P: AsRef<Utf8Path>>(path: P) -> Result<(), RemoveFileError> {
    let path = path.as_ref();
    std::fs::remove_file(path).context(RemoveFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to write {path}"))]
pub struct WriteFileError {
    pub path: Utf8PathBuf,
    pub source: std::io::Error,
}

pub fn write<P: AsRef<Utf8Path>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> Result<(), WriteFileError> {
    let path = path.as_ref();
    std::fs::write(path, contents).context(WriteFileSnafu { path })
}
