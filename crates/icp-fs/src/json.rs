use icp::prelude::*;
use serde::Serialize;
use snafu::prelude::*;

use icp::fs::read;

#[derive(Snafu, Debug)]
pub enum LoadJsonFileError {
    #[snafu(display("failed to parse {path} as json"))]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(transparent)]
    Read { source: icp::fs::Error },
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
    Write { source: icp::fs::Error },
}

pub fn save_json_file<T: Serialize>(
    path: impl AsRef<Path>,
    value: &T,
) -> Result<(), SaveJsonFileError> {
    let path = path.as_ref();
    let content = serde_json::to_string_pretty(&value).context(SerializeSnafu { path })?;
    icp::fs::write_string(path, &content)?;
    Ok(())
}
