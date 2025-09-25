use crate::lock::{
    AcquireWriteLockError, OpenFileForWriteLockError, ReadWithLockError, RwFileLock,
};
use camino::{Utf8Path as Path, Utf8PathBuf as PathBuf};
use serde::ser::Serialize;
use snafu::{ResultExt, Snafu};
use std::io::Write;

pub fn load_json_with_lock<T: serde::de::DeserializeOwned>(
    path: impl AsRef<Path>,
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
        path: PathBuf,
    },

    #[snafu(transparent)]
    ReadWithLock { source: ReadWithLockError },
}

pub fn save_json_with_lock<T: Serialize>(
    path: impl AsRef<Path>,
    data: &T,
) -> Result<(), SaveJsonWithLockError> {
    let path = path.as_ref();
    let mut lock = RwFileLock::open_for_write(path)?;
    let mut guard = lock.acquire_write_lock()?;

    let content = serde_json::to_vec_pretty(data).context(SerializeSnafu)?;

    guard.set_len(0).context(TruncateSnafu { path })?;
    guard.write_all(&content).context(WriteSnafu { path })?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum SaveJsonWithLockError {
    #[snafu(transparent)]
    OpenFileForWriteLock { source: OpenFileForWriteLockError },

    #[snafu(transparent)]
    AcquireWriteLock { source: AcquireWriteLockError },

    #[snafu(display("failed to serialize data to json"))]
    Serialize { source: serde_json::Error },

    #[snafu(display("failed to truncate file at {path}"))]
    Truncate {
        source: std::io::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to write to file at {path}"))]
    Write {
        source: std::io::Error,
        path: PathBuf,
    },
}
