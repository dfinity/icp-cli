use crate::lock::{ReadWithLockError, RwFileLock};
use camino::{Utf8Path, Utf8PathBuf};
use snafu::{ResultExt, Snafu};

pub fn load_json_with_lock<T: serde::de::DeserializeOwned>(
    path: impl AsRef<Utf8Path>,
) -> Result<Option<T>, LoadJsonWithLockError> {
    let path = path.as_ref();

    if !path.exists() {
        return Ok(None);
    }

    let content = RwFileLock::read(path)?;

    // On Windows, we can't delete a file that is locked for writing,
    // so instead we truncate it. We'll treat a truncated file
    // as if it doesn't exist.
    if content.is_empty() {
        return Ok(None);
    }

    let parsed = serde_json::from_slice(&content).context(ParseSnafu { path })?;
    Ok(Some(parsed))
}

#[derive(Debug, Snafu)]
pub enum LoadJsonWithLockError {
    #[snafu(display("failed to parse {path} as json"))]
    Parse {
        source: serde_json::Error,
        path: Utf8PathBuf,
    },

    #[snafu(transparent)]
    ReadWithLock { source: ReadWithLockError },
}
