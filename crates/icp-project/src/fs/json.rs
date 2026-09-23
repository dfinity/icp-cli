use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use crate::{
    fs::{read, write_string},
    prelude::*,
};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(transparent)]
    Io { source: crate::fs::IoError },

    #[snafu(display("failed to parse json file at {path}"))]
    Parse {
        source: serde_json::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to serialize json file to {path}"))]
    Serialize {
        source: serde_json::Error,
        path: PathBuf,
    },
}

pub fn load<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, Error> {
    serde_json::from_slice(&read(path)?).context(ParseSnafu { path })
}

pub fn load_or_default<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> Result<T, Error> {
    if path.exists() {
        load(path)
    } else {
        Ok(T::default())
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    write_string(
        path,
        &serde_json::to_string_pretty(&value).context(SerializeSnafu { path })?,
    )?;

    Ok(())
}
