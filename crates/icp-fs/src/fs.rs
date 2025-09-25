use camino::{Utf8Path as Path, Utf8PathBuf as PathBuf};
use snafu::prelude::*;

#[derive(Snafu, Debug)]
#[snafu(display("failed to create directory {path} and parents"))]
pub struct CreateDirAllError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn create_dir_all(path: impl AsRef<Path>) -> Result<(), CreateDirAllError> {
    let path = path.as_ref();
    std::fs::create_dir_all(path).context(CreateDirAllSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to read file {path}"))]
pub struct ReadFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn read<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, ReadFileError> {
    let path = path.as_ref();
    std::fs::read(path).context(ReadFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to read text file {path}"))]
pub struct ReadToStringError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> Result<String, ReadToStringError> {
    let path = path.as_ref();
    std::fs::read_to_string(path).context(ReadToStringSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove directory {path} and contents"))]
pub struct RemoveDirAllError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn remove_dir_all(path: impl AsRef<Path>) -> Result<(), RemoveDirAllError> {
    let path = path.as_ref();
    std::fs::remove_dir_all(path).context(RemoveDirAllSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to remove file {path}"))]
pub struct RemoveFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> Result<(), RemoveFileError> {
    let path = path.as_ref();
    std::fs::remove_file(path).context(RemoveFileSnafu { path })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to write {path}"))]
pub struct WriteFileError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<(), WriteFileError> {
    let path = path.as_ref();
    std::fs::write(path, contents).context(WriteFileSnafu { path })
}
