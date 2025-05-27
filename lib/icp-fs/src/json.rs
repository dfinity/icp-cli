use crate::fs::{ReadFileError, WriteFileError, read};
use serde::Serialize;
use snafu::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Snafu, Debug)]
pub enum LoadJsonFileError {
    #[snafu(display("failed to parse {} as json", path.display()))]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(transparent)]
    Read { source: ReadFileError },
}

pub fn load_json_file<T: for<'a> serde::de::Deserialize<'a>>(
    path: &Path,
) -> Result<T, LoadJsonFileError> {
    let content = read(path)?;

    serde_json::from_slice(content.as_ref()).context(ParseSnafu { path })
}

#[derive(Snafu, Debug)]
pub enum SaveJsonFileError {
    #[snafu(display("failed to serialize json for {}", path.display()))]
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(transparent)]
    Write { source: WriteFileError },
}

pub fn save_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), SaveJsonFileError> {
    let content = serde_json::to_string_pretty(&value).context(SerializeSnafu { path })?;
    crate::fs::write(path, content)?;
    Ok(())
}
