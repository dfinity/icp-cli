use camino::{Utf8Path as Path, Utf8PathBuf as PathBuf};
use fd_lock::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use snafu::prelude::*;
use std::fs::{File, OpenOptions};
use std::io::Read;

pub struct RwFileLock {
    lock: RwLock<File>,
    path: PathBuf,
}

impl RwFileLock {
    pub fn new(file: File, path: &Path) -> Self {
        Self {
            lock: RwLock::new(file),
            path: path.to_path_buf(),
        }
    }

    pub fn rwlock_mut(&mut self) -> &mut RwLock<File> {
        &mut self.lock
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open_for_read(path: impl AsRef<Path>) -> Result<Self, OpenFileForReadLockError> {
        let path = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .context(OpenFileForReadLockSnafu { path })?;
        Ok(Self::new(file, path))
    }

    pub fn acquire_read_lock(&mut self) -> Result<RwLockReadGuard<'_, File>, AcquireReadLockError> {
        self.lock.read().context(AcquireReadLockSnafu {
            path: self.path.clone(),
        })
    }

    pub fn read(path: impl AsRef<Path>) -> Result<Vec<u8>, ReadWithLockError> {
        let path = path.as_ref();
        let mut lock = Self::open_for_read(path)?;
        let guard = lock.acquire_read_lock()?;

        let mut buf = vec![];
        (&*guard)
            .read_to_end(&mut buf)
            .context(ReadFileSnafu { path })?;

        Ok(buf)
    }

    pub fn open_for_write(path: impl AsRef<Path>) -> Result<Self, OpenFileForWriteLockError> {
        let path = path.as_ref();
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(path)
            .context(OpenFileForWriteLockSnafu { path })?;
        Ok(Self::new(file, path))
    }

    pub fn acquire_write_lock(
        &mut self,
    ) -> Result<RwLockWriteGuard<'_, File>, AcquireWriteLockError> {
        self.lock.write().context(AcquireWriteLockSnafu {
            path: self.path.clone(),
        })
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to open file for read lock at {path}"))]
pub struct OpenFileForReadLockError {
    source: std::io::Error,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to open file for write lock at {path}"))]
pub struct OpenFileForWriteLockError {
    source: std::io::Error,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to acquire read lock on {path}"))]
pub struct AcquireReadLockError {
    source: std::io::Error,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to acquire write lock on {path}"))]
pub struct AcquireWriteLockError {
    source: std::io::Error,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
pub enum ReadWithLockError {
    #[snafu(transparent)]
    AcquireReadLock { source: AcquireReadLockError },

    #[snafu(transparent)]
    OpenFileForReadLock { source: OpenFileForReadLockError },

    #[snafu(display("failed to read file at {path}"))]
    ReadFile {
        source: std::io::Error,
        path: PathBuf,
    },
}
