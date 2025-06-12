use crate::config::model::network_descriptor::NetworkDescriptorModel;
use camino::Utf8PathBuf;
use fd_lock::RwLockWriteGuard;
use icp_fs::fs::remove_file;
use icp_fs::lock::{AcquireWriteLockError, RwFileLock};
use snafu::prelude::*;
use std::fs::File;
use std::io::Seek;
use std::io::Write;

pub struct NetworkDescriptorWriter<'lock> {
    write_guard: RwLockWriteGuard<'lock, File>,
    path: Utf8PathBuf,
}

impl<'lock> NetworkDescriptorWriter<'lock> {
    pub fn acquire(file_lock: &'lock mut RwFileLock) -> Result<Self, AcquireWriteLockError> {
        let path = file_lock.path().to_path_buf();
        let write_guard = file_lock.acquire_write_lock()?;
        Ok(Self { write_guard, path })
    }

    pub fn truncate(&mut self) -> Result<(), TruncateFileError> {
        self.write_guard
            .set_len(0)
            .context(TruncateFileSnafu { path: &self.path })?;
        self.write_guard
            .seek(std::io::SeekFrom::Start(0))
            .context(TruncateFileSnafu { path: &self.path })?;
        Ok(())
    }

    pub fn write(
        &mut self,
        descriptor: &NetworkDescriptorModel,
    ) -> Result<(), WriteDescriptorError> {
        let content = serde_json::to_string_pretty(descriptor).unwrap();
        write!(*self.write_guard, "{}", content).context(WriteSnafu { path: &self.path })?;
        Ok(())
    }

    /// "Cleans" a network descriptor.
    /// On Unix, this means removing the file.
    /// On Windows, it means truncating the file to zero length since it can't
    /// be removed while holding a lock on it.
    pub fn cleanup(&mut self) {
        #[cfg(windows)]
        let _ = self.truncate();

        #[cfg(unix)]
        let _ = remove_file(&self.path);
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to truncate file at {path}"))]
pub struct TruncateFileError {
    source: std::io::Error,
    path: Utf8PathBuf,
}

#[derive(Debug, Snafu)]
pub enum WriteDescriptorError {
    Write {
        source: std::io::Error,
        path: Utf8PathBuf,
    },
}
