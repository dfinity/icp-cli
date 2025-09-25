use camino::{Utf8Path, Utf8PathBuf};
use snafu::prelude::*;

use crate::fs::{ReadFileError, read};

#[derive(Snafu, Debug)]
pub enum LoadYamlFileError {
    #[snafu(display("failed to parse {path} as yaml"))]
    Parse {
        path: Utf8PathBuf,
        source: serde_yaml::Error,
    },

    #[snafu(transparent)]
    Read { source: ReadFileError },
}

pub fn load_yaml_file<T: for<'a> serde::de::Deserialize<'a>>(
    path: impl AsRef<Utf8Path>,
) -> Result<T, LoadYamlFileError> {
    let path = path.as_ref();
    let content = read(path)?;

    serde_yaml::from_slice(content.as_ref()).context(ParseSnafu { path })
}
