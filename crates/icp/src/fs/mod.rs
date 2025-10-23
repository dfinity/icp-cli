use std::{
    fmt::Display,
    io::{self, ErrorKind},
};

use crate::prelude::*;

pub mod json;
pub mod lock;
pub mod yaml;

#[derive(Debug, thiserror::Error)]
pub struct Error {
    path: PathBuf,

    #[source]
    err: io::Error,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.err.kind()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}: {}", self.path, self.err))
    }
}

pub fn create_dir_all(path: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|err| Error {
        path: path.into(),
        err,
    })
}

pub fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|err| Error {
        path: path.into(),
        err,
    })
}

pub fn read_to_string(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|err| Error {
        path: path.into(),
        err,
    })
}

pub fn remove_dir_all(path: &Path) -> Result<(), Error> {
    std::fs::remove_dir_all(path).map_err(|err| Error {
        path: path.into(),
        err,
    })
}

pub fn remove_file(path: &Path) -> Result<(), Error> {
    std::fs::remove_file(path).map_err(|err| Error {
        path: path.into(),
        err,
    })
}

pub fn write(path: &Path, contents: &[u8]) -> Result<(), Error> {
    std::fs::write(path, contents).map_err(|err| Error {
        path: path.into(),
        err,
    })
}

pub fn write_string(path: &Path, contents: &str) -> Result<(), Error> {
    std::fs::write(path, contents.as_bytes()).map_err(|err| Error {
        path: path.into(),
        err,
    })
}
