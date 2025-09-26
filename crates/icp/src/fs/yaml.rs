use serde::Deserialize;

use crate::{fs::read, prelude::*};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] crate::fs::Error),

    #[error("failed to parse json file at {path}")]
    Parse {
        path: PathBuf,

        #[source]
        err: serde_yaml::Error,
    },
}

pub fn load<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, Error> {
    serde_yaml::from_slice(&read(path)?).map_err(|err| Error::Parse {
        path: path.into(),
        err,
    })
}
