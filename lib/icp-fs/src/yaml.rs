use std::path::{Path, PathBuf};

use snafu::prelude::*;

use crate::fs::{ReadFileError, read};

#[derive(Snafu, Debug)]
pub enum LoadYamlFileError {
    #[snafu(display("failed to parse {} as yaml", path.display()))]
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },

    #[snafu(transparent)]
    Read { source: ReadFileError },
}

pub fn load_yaml_file<T: for<'a> serde::de::Deserialize<'a>>(
    path: &Path,
) -> Result<T, LoadYamlFileError> {
    let content = read(path)?;

    serde_yaml::from_slice(content.as_ref()).context(ParseSnafu { path })
}
