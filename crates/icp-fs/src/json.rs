use crate::fs::{ReadFileError, WriteFileError, read};
use camino::{Utf8Path as Path, Utf8PathBuf as PathBuf};
use serde::Serialize;
use snafu::prelude::*;

#[derive(Snafu, Debug)]
pub enum LoadJsonFileError {
    #[snafu(display("failed to parse {path} as json"))]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(transparent)]
    Read { source: ReadFileError },
}

pub fn load_json_file<T: for<'a> serde::de::Deserialize<'a>>(
    path: impl AsRef<Path>,
) -> Result<T, LoadJsonFileError> {
    let path = path.as_ref();
    let content = read(path)?;

    serde_json::from_slice(content.as_ref()).context(ParseSnafu { path })
}

#[derive(Snafu, Debug)]
pub enum SaveJsonFileError {
    #[snafu(display("failed to serialize json for {path}"))]
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(transparent)]
    Write { source: WriteFileError },
}

pub fn save_json_file<T: Serialize>(
    path: impl AsRef<Path>,
    value: &T,
) -> Result<(), SaveJsonFileError> {
    let path = path.as_ref();
    let content = serde_json::to_string_pretty(&value).context(SerializeSnafu { path })?;
    crate::fs::write(path, content)?;
    Ok(())
}
