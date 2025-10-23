use crate::prelude::*;
use snafu::{ResultExt, Snafu};
use std::{fs::File, io, ops::Deref};
use tokio::{
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
    task::spawn_blocking,
};

pub struct DirectoryStructureLock<T: PathsAccess> {
    paths_access: T,
    lock_file: RwLock<File>,
    lock_path: PathBuf,
}

pub trait PathsAccess: Send + Sync + 'static {
    fn lock_file(&self) -> PathBuf;
}

impl<T: PathsAccess> DirectoryStructureLock<T> {
    pub fn get_or_create(paths_access: T) -> Result<Self, LockError> {
        let lock_path = paths_access.lock_file();
        crate::fs::create_dir_all(lock_path.parent().unwrap())?;
        let lock_file =
            File::create(&lock_path).context(OpenLockFileFailedSnafu { path: &lock_path })?;
        Ok(Self {
            paths_access,
            lock_file: RwLock::const_new(lock_file),
            lock_path,
        })
    }

    pub async fn read(self) -> Result<DirectoryStructureGuardOwned<T>, LockError> {
        spawn_blocking(move || {
            let lock_file = self.lock_file.into_inner();
            lock_file.lock_shared().context(LockFailedSnafu {
                lock_path: self.lock_path,
            })?;
            Ok(DirectoryStructureGuardOwned {
                paths_access: self.paths_access,
                guard: lock_file,
            })
        })
        .await
        .unwrap()
    }

    pub async fn write(self) -> Result<DirectoryStructureGuardOwned<T>, LockError> {
        spawn_blocking(move || {
            let lock_file = self.lock_file.into_inner();
            lock_file.lock().context(LockFailedSnafu {
                lock_path: self.lock_path,
            })?;
            Ok(DirectoryStructureGuardOwned {
                paths_access: self.paths_access,
                guard: lock_file,
            })
        })
        .await
        .unwrap()
    }

    pub async fn read_ref(&self) -> Result<DirectoryStructureGuardReadRef<'_, T>, LockError> {
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

        Ok(DirectoryStructureGuardReadRef {
            paths_access: &self.paths_access,
            guard,
        })
    }

    pub async fn write_ref(&self) -> Result<DirectoryStructureGuardWriteRef<'_, T>, LockError> {
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

        Ok(DirectoryStructureGuardWriteRef {
            paths_access: &self.paths_access,
            guard,
        })
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

pub struct DirectoryStructureGuardReadRef<'a, T> {
    paths_access: &'a T,
    guard: RwLockReadGuard<'a, File>,
}

impl<'a, T> Deref for DirectoryStructureGuardReadRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.paths_access
    }
}

impl<'a, T> Drop for DirectoryStructureGuardReadRef<'a, T> {
    fn drop(&mut self) {
        _ = self.guard.unlock();
    }
}

pub struct DirectoryStructureGuardWriteRef<'a, T> {
    paths_access: &'a T,
    guard: RwLockWriteGuard<'a, File>,
}

impl<'a, T> Deref for DirectoryStructureGuardWriteRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.paths_access
    }
}

impl<'a, T> Drop for DirectoryStructureGuardWriteRef<'a, T> {
    fn drop(&mut self) {
        _ = self.guard.unlock();
    }
}

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
