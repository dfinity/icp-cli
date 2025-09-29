use serde::{Deserialize, Serialize};

use crate::{
    fs::{read, write_string},
    prelude::*,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] crate::fs::Error),

    #[error("failed to parse json file at {path}")]
    Parse {
        path: PathBuf,

        #[source]
        err: serde_json::Error,
    },

    #[error("failed to serialize json file to {path}")]
    Serialize {
        path: PathBuf,

        #[source]
        err: serde_json::Error,
    },
}

pub fn load<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, Error> {
    serde_json::from_slice(&read(path)?).map_err(|err| Error::Parse {
        path: path.into(),
        err,
    })
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    write_string(
        path,
        &serde_json::to_string_pretty(&value).map_err(|err| Error::Parse {
            path: path.into(),
            err,
        })?,
    )?;

    Ok(())
}
