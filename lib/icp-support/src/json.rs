use crate::fs::{ReadFileError, read};
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
