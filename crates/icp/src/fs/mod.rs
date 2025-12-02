use snafu::prelude::*;
use std::io::{self, ErrorKind};

use crate::prelude::*;

pub mod json;
pub mod lock;
pub mod yaml;

#[derive(Debug, Snafu)]
#[snafu(display("Filesystem operation failed at {path}"))]
pub struct IoError {
    source: io::Error,
    path: PathBuf,
}

impl IoError {
    pub fn kind(&self) -> ErrorKind {
        self.source.kind()
    }
}

// impl Display for Error {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_fmt(format_args!("{}: {}", self.path, self.err))
//     }
// }

pub fn create_dir_all(path: &Path) -> Result<(), IoError> {
    std::fs::create_dir_all(path).context(IoSnafu { path })
}

pub fn read(path: &Path) -> Result<Vec<u8>, IoError> {
    std::fs::read(path).context(IoSnafu { path })
}

pub fn read_to_string(path: &Path) -> Result<String, IoError> {
    std::fs::read_to_string(path).context(IoSnafu { path })
}

pub fn remove_dir_all(path: &Path) -> Result<(), IoError> {
    std::fs::remove_dir_all(path).context(IoSnafu { path })
}

pub fn remove_file(path: &Path) -> Result<(), IoError> {
    std::fs::remove_file(path).context(IoSnafu { path })
}

pub fn write(path: &Path, contents: &[u8]) -> Result<(), IoError> {
    std::fs::write(path, contents).context(IoSnafu { path })
}

pub fn write_string(path: &Path, contents: &str) -> Result<(), IoError> {
    std::fs::write(path, contents.as_bytes()).context(IoSnafu { path })
}
