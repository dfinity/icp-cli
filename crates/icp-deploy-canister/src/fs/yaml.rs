use serde::Deserialize;
use snafu::prelude::*;

use crate::{fs::read, prelude::*};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(transparent)]
    Io { source: crate::fs::IoError },

    #[snafu(display("failed to parse yaml file at {path}"))]
    Parse {
        source: serde_yaml::Error,
        path: PathBuf,
    },
}

pub fn load<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, Error> {
    serde_yaml::from_slice::<T>(&read(path)?).context(ParseSnafu { path })
}
