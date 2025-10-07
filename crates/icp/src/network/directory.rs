use std::io::ErrorKind;

use snafu::prelude::*;

use crate::{
    fs::{create_dir_all, json},
    network::{
        config::NetworkDescriptorModel,
        lock::{AcquireWriteLockError, OpenFileForWriteLockError, RwFileLock},
        managed::descriptor::{
            FixedPortLock, NetworkDescriptorCleaner, NetworkDescriptorWriter, NetworkLock,
            TruncateFileError, WriteDescriptorError,
        },
        structure::NetworkDirectoryStructure,
    },
    prelude::*,
};

pub struct NetworkDirectory {
    pub network_name: String,
    pub structure: NetworkDirectoryStructure,
}

impl NetworkDirectory {
    pub fn new(network_name: &str, network_root: &Path, port_descriptor_dir: &Path) -> Self {
        let network_name = network_name.to_string();
        let structure = NetworkDirectoryStructure::new(network_root, port_descriptor_dir);

        Self {
            network_name,
            structure,
        }
    }
}

impl NetworkDirectory {
    pub fn structure(&self) -> &NetworkDirectoryStructure {
        &self.structure
    }

    pub fn ensure_exists(&self) -> Result<(), crate::fs::Error> {
        create_dir_all(&self.structure.network_root)?;
        create_dir_all(&self.structure.port_descriptor_dir)
    }

    pub fn load_network_descriptor(&self) -> Result<Option<NetworkDescriptorModel>, json::Error> {
        json::load(&self.structure.network_descriptor_path()).or_else(|err| match err {
            // Default to empty
            json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(None),

            // Other
            _ => Err(err),
        })
    }

    pub fn load_port_descriptor(
        &self,
        port: u16,
    ) -> Result<Option<NetworkDescriptorModel>, json::Error> {
        json::load(&self.structure.port_descriptor_path(port)).or_else(|err| match err {
            // Default to empty
            json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(None),

            // Other
            _ => Err(err),
        })
    }

    pub fn open_network_lock_file(&self) -> Result<NetworkLock, OpenFileForWriteLockError> {
        let rwlock = RwFileLock::open_for_write(self.structure.network_lock_path())?;
        Ok(NetworkLock::new(rwlock, &self.network_name))
    }

    pub fn open_port_lock_file(
        &self,
        port: u16,
    ) -> Result<FixedPortLock, OpenFileForWriteLockError> {
        let rwlock = RwFileLock::open_for_write(self.structure.port_lock_path(port))?;
        let port_descriptor_path = self.structure.port_descriptor_path(port);
        Ok(FixedPortLock::new(rwlock, &port_descriptor_path, port))
    }

    fn open_network_descriptor_for_writelock(
        &self,
    ) -> Result<RwFileLock, OpenFileForWriteLockError> {
        RwFileLock::open_for_write(self.structure.network_descriptor_path())
    }

    fn open_port_descriptor_for_writelock(
        &self,
        port: u16,
    ) -> Result<RwFileLock, OpenFileForWriteLockError> {
        RwFileLock::open_for_write(self.structure.port_descriptor_path(port))
    }

    pub fn save_network_descriptors(
        &self,
        descriptor: &NetworkDescriptorModel,
    ) -> Result<NetworkDescriptorCleaner, SaveNetworkDescriptorError> {
        let mut network_lock = self.open_network_descriptor_for_writelock()?;
        let mut network_writer = NetworkDescriptorWriter::acquire(&mut network_lock)?;

        let mut port_lock: Option<RwFileLock>;
        let mut port_writer = None;

        if let Some(port) = descriptor.gateway_port() {
            // Must place in `port_lock` first, so we can borrow it for `port_writer` without moving
            port_lock = Some(self.open_port_descriptor_for_writelock(port)?);
            port_writer = Some(NetworkDescriptorWriter::acquire(
                port_lock.as_mut().unwrap(),
            )?);
        }

        // Avoid having the network descriptor refer to
        // a port descriptor that is not yet written.
        network_writer.truncate()?;
        if let Some(ref mut fixed_port_writer) = port_writer {
            fixed_port_writer.truncate()?;
            fixed_port_writer.write(descriptor)?;
        }
        network_writer.write(descriptor)?;

        Ok(NetworkDescriptorCleaner::new(
            self,
            descriptor.gateway_port(),
        ))
    }

    pub fn cleanup_project_network_descriptor(&self) -> Result<(), CleanupNetworkDescriptorError> {
        let mut file = self.open_network_descriptor_for_writelock()?;
        let mut writer = NetworkDescriptorWriter::acquire(&mut file)?;
        writer.cleanup();

        Ok(())
    }

    pub fn cleanup_port_descriptor(
        &self,
        gateway_port: Option<u16>,
    ) -> Result<(), CleanupNetworkDescriptorError> {
        if let Some(port) = gateway_port {
            let mut file = self.open_port_descriptor_for_writelock(port)?;
            let mut writer = NetworkDescriptorWriter::acquire(&mut file)?;
            writer.cleanup();
        }
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum SaveNetworkDescriptorError {
    #[snafu(transparent)]
    AcquireWriteLock { source: AcquireWriteLockError },

    #[snafu(transparent)]
    OpenFileForWriteLock { source: OpenFileForWriteLockError },

    #[snafu(transparent)]
    TruncateFile { source: TruncateFileError },

    #[snafu(transparent)]
    WriteDescriptor { source: WriteDescriptorError },

    #[snafu(display("failed to obtain descriptor for project network descriptor"))]
    ObtainProjectNetworkDescriptorLock { path: PathBuf },
}

#[derive(Debug, Snafu)]
pub enum CleanupNetworkDescriptorError {
    #[snafu(transparent)]
    OpenFile { source: OpenFileForWriteLockError },

    #[snafu(transparent)]
    AcquireWriteLock { source: AcquireWriteLockError },
}
