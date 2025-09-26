use std::{
    fs::File,
    io::{Seek, Write},
};

use fd_lock::RwLockWriteGuard;
use icp::{
    fs::{json, remove_file},
    prelude::*,
};
use icp_fs::lock::{AcquireWriteLockError, RwFileLock};
use snafu::prelude::*;

use crate::{NetworkDirectory, config::NetworkDescriptorModel};

pub struct NetworkLock {
    file_lock: RwFileLock,
    network_name: String,
}

impl NetworkLock {
    pub fn new(file_lock: RwFileLock, network_name: &str) -> Self {
        Self {
            file_lock,
            network_name: network_name.to_string(),
        }
    }

    pub fn try_acquire(&mut self) -> Result<LockFileClaim<'_>, ProjectNetworkAlreadyRunningError> {
        let path = self.file_lock.path().to_owned();
        let guard = self.file_lock.rwlock_mut().try_write().map_err(|_| {
            ProjectNetworkAlreadyRunningError {
                network: self.network_name.clone(),
            }
        })?;
        Ok(LockFileClaim::new(path, guard))
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("the {network} network for this project is already running"))]
pub struct ProjectNetworkAlreadyRunningError {
    pub network: String,
}

/// Encapsulates our claim on a lock file.
/// In this context, a lock file is a 0-length file used to manage
/// exclusive access to a resource, such as a network port.
/// Deletes the lock and releases the lock when dropped.
pub struct LockFileClaim<'a> {
    path: PathBuf,
    _guard: RwLockWriteGuard<'a, File>,
}

impl<'a> LockFileClaim<'a> {
    pub fn new(path: impl AsRef<Path>, guard: RwLockWriteGuard<'a, File>) -> Self {
        let path = path.as_ref().to_path_buf();
        Self {
            path,
            _guard: guard,
        }
    }
}

impl Drop for LockFileClaim<'_> {
    fn drop(&mut self) {
        // On Windows, the file can't be removed while it's locked,
        // so we just leave it in place in order to avoid potential
        // race conditions.
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

pub struct FixedPortLock {
    file_lock: RwFileLock,
    port_descriptor_path: PathBuf,
    port: u16,
}

impl FixedPortLock {
    pub fn new(file_lock: RwFileLock, port_descriptor_path: &Path, port: u16) -> Self {
        Self {
            file_lock,
            port_descriptor_path: port_descriptor_path.to_path_buf(),
            port,
        }
    }

    pub fn try_acquire(
        &mut self,
    ) -> Result<LockFileClaim<'_>, AnotherProjectRunningOnSamePortError> {
        let lock_path = self.file_lock.path().to_path_buf();

        let guard = self.file_lock.rwlock_mut().try_write().map_err(|_| {
            let network_descriptor = json::load(&self.port_descriptor_path)
                .ok()
                .flatten()
                .map(Box::new);
            AnotherProjectRunningOnSamePortError {
                network_descriptor,
                port: self.port,
            }
        })?;

        Ok(LockFileClaim::new(lock_path, guard))
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("port {port} is in use by the {} network of the project at '{}'",
  network_descriptor.as_ref().map(|nd| nd.network.clone()).unwrap_or_else(|| "<unknown>".to_string()),
  network_descriptor.as_ref().map(|nd| nd.project_dir.to_string()).unwrap_or_else(|| "<unknown>".to_string())))]
pub struct AnotherProjectRunningOnSamePortError {
    pub network_descriptor: Option<Box<NetworkDescriptorModel>>,
    pub port: u16,
}

pub struct NetworkDescriptorCleaner<'a> {
    network_directory: &'a NetworkDirectory,
    gateway_port: Option<u16>,
}

impl<'a> NetworkDescriptorCleaner<'a> {
    pub fn new(network_directory: &'a NetworkDirectory, gateway_port: Option<u16>) -> Self {
        Self {
            network_directory,
            gateway_port,
        }
    }
}

impl Drop for NetworkDescriptorCleaner<'_> {
    fn drop(&mut self) {
        let _ = self.network_directory.cleanup_project_network_descriptor();
        let _ = self
            .network_directory
            .cleanup_port_descriptor(self.gateway_port);
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to truncate file at {path}"))]
pub struct TruncateFileError {
    source: std::io::Error,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
pub enum WriteDescriptorError {
    Write {
        source: std::io::Error,
        path: PathBuf,
    },
}

pub struct NetworkDescriptorWriter<'lock> {
    write_guard: RwLockWriteGuard<'lock, File>,
    path: PathBuf,
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
        write!(
            *self.write_guard,
            "{}",
            serde_json::to_string_pretty(descriptor).unwrap(),
        )
        .context(WriteSnafu { path: &self.path })?;

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
