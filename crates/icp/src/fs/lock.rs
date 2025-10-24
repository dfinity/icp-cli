//! File locking abstractions to make directory locks easy and safe.
//!
//! Directory structures are typically represented by a struct containing the directory path,
//! which then has methods for getting files or directories within it. This struct should implement
//! `PathsAccess`, and then instead of passing it around directly, it should be stored in a
//! [`DirectoryStructureLock`]. This ensures that paths cannot be accessed without holding the appropriate
//! lock.
//!
//! Temporary locks can be acquired using the `with_read` and `with_write` methods, which take
//! async closures. For locks that should be stored in a structure, `into_read` and `into_write` can be used
//! to convert the lock into an owned guard.
//!
//! When making low-level functions that might be composed into higher-level operations, these functions should
//! typically take `LRead<&T>` or `LWrite<&T>` parameters, rather than `&T`. This makes sure the composed function
//! will demand the right kind of lock, when writes are hidden in what looks at first glance like a read operation.

use crate::{fs, prelude::*};
use snafu::{ResultExt, Snafu};
use std::{fs::File, io, ops::Deref};
use tokio::{sync::RwLock, task::spawn_blocking};

/// Directory lock ensuring safe concurrency around filesystem operations.
pub struct DirectoryStructureLock<T: PathsAccess> {
    paths_access: T,
    lock_file: RwLock<File>,
    lock_path: PathBuf,
}

/// A directory structure, typically with methods to access specific paths within it.
///
/// One file within it is selected as the file for advisory locks.
pub trait PathsAccess: Send + Sync + 'static {
    /// Path to the canonical file for locking the directory structure. Usually `$dir/.lock`.
    fn lock_file(&self) -> PathBuf;
}

impl<T: PathsAccess> DirectoryStructureLock<T> {
    /// Creates a new lock, implicitly calling [`fs::create_dir_all`] on the parent.
    pub fn open_or_create(paths_access: T) -> Result<Self, LockError> {
        let lock_path = paths_access.lock_file();
        fs::create_dir_all(lock_path.parent().unwrap())?;
        let lock_file =
            File::create(&lock_path).context(OpenLockFileFailedSnafu { path: &lock_path })?;
        Ok(Self {
            paths_access,
            lock_file: RwLock::const_new(lock_file),
            lock_path,
        })
    }

    /// Converts the lock structure into an owned read-lock.
    pub async fn into_read(self) -> Result<DirectoryStructureGuardOwned<LRead<T>>, LockError> {
        spawn_blocking(move || {
            let lock_file = self.lock_file.into_inner();
            lock_file.lock_shared().context(LockFailedSnafu {
                lock_path: self.lock_path,
            })?;
            Ok(DirectoryStructureGuardOwned {
                paths_access: LRead(self.paths_access),
                guard: lock_file,
            })
        })
        .await
        .unwrap()
    }

    /// Converts the lock structure into an owned write-lock.
    pub async fn into_write(self) -> Result<DirectoryStructureGuardOwned<LWrite<T>>, LockError> {
        spawn_blocking(move || {
            let lock_file = self.lock_file.into_inner();
            lock_file.lock().context(LockFailedSnafu {
                lock_path: self.lock_path,
            })?;
            Ok(DirectoryStructureGuardOwned {
                paths_access: LWrite(self.paths_access),
                guard: lock_file,
            })
        })
        .await
        .unwrap()
    }

    /// Accesses the directory structure under a read lock.
    pub async fn with_read<R>(&self, f: impl AsyncFnOnce(LRead<&T>) -> R) -> Result<R, LockError> {
        let guard = self.lock_file.read().await;
        let lock_file = guard.try_clone().context(HandleCloneFailedSnafu {
            path: &self.lock_path,
        })?;
        spawn_blocking(move || lock_file.lock_shared())
            .await
            .unwrap()
            .context(LockFailedSnafu {
                lock_path: &self.lock_path,
            })?;
        let ret = f(LRead(&self.paths_access)).await;
        guard.unlock().context(LockFailedSnafu {
            lock_path: &self.lock_path,
        })?;
        Ok(ret)
    }

    /// Accesses the directory structure under a write lock.
    pub async fn with_write<R>(
        &self,
        f: impl AsyncFnOnce(LWrite<&T>) -> R,
    ) -> Result<R, LockError> {
        let guard = self.lock_file.write().await;
        let lock_file = guard.try_clone().context(HandleCloneFailedSnafu {
            path: &self.lock_path,
        })?;
        spawn_blocking(move || lock_file.lock())
            .await
            .unwrap()
            .context(LockFailedSnafu {
                lock_path: &self.lock_path,
            })?;
        let ret = f(LWrite(&self.paths_access)).await;
        guard.unlock().context(LockFailedSnafu {
            lock_path: &self.lock_path,
        })?;
        Ok(ret)
    }
}

#[derive(Debug, Snafu)]
pub enum LockError {
    #[snafu(transparent)]
    CreateDirFailed { source: crate::fs::Error },
    #[snafu(display("failed to create or open lock file '{path}'"))]
    OpenLockFileFailed { source: io::Error, path: PathBuf },
    #[snafu(display("failed to lock the file '{lock_path}'"))]
    LockFailed {
        source: io::Error,
        lock_path: PathBuf,
    },
    #[snafu(display("failed to clone lock file handle '{path}'"))]
    HandleCloneFailed { source: io::Error, path: PathBuf },
}

/// File lock guard. Do not use as a temporary in an expression - if you are making a temporary lock, use `with_*`.
pub struct DirectoryStructureGuardOwned<T> {
    paths_access: T,
    guard: File,
}

impl<T> Deref for DirectoryStructureGuardOwned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.paths_access
    }
}

impl<T> Drop for DirectoryStructureGuardOwned<T> {
    fn drop(&mut self) {
        _ = self.guard.unlock();
    }
}

pub struct LRead<T>(T);
pub struct LWrite<T>(T);

impl<T> Deref for LRead<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Deref for LWrite<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T> LWrite<&'a T> {
    pub fn read(&self) -> LRead<&'a T> {
        LRead(self.0)
    }
}

impl<T> LWrite<T> {
    pub fn as_ref(&self) -> LWrite<&T> {
        LWrite(&self.0)
    }
}

impl<T> LRead<T> {
    pub fn as_ref(&self) -> LRead<&T> {
        LRead(&self.0)
    }
}
