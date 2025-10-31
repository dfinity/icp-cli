use std::io::ErrorKind;

use snafu::prelude::*;
use sysinfo::Pid;

use crate::{
    fs::{
        create_dir_all, json,
        lock::{DirectoryStructureLock, LWrite, LockError, PathsAccess},
        read_to_string,
    },
    network::config::NetworkDescriptorModel,
    prelude::*,
};

#[derive(Clone)]
pub struct NetworkDirectory {
    pub network_name: String,
    pub network_root: PathBuf,
    pub port_descriptor_dir: PathBuf,
}

impl NetworkDirectory {
    pub fn new(network_name: &str, network_root: &Path, port_descriptor_dir: &Path) -> Self {
        Self {
            network_name: network_name.to_owned(),
            network_root: network_root.to_path_buf(),
            port_descriptor_dir: port_descriptor_dir.to_path_buf(),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum LoadNetworkFileError {
    #[snafu(transparent)]
    LockFileError { source: LockError },

    #[snafu(transparent)]
    JsonLoadError { source: json::Error },
}

impl NetworkDirectory {
    pub fn ensure_exists(&self) -> Result<(), crate::fs::Error> {
        // Network root
        create_dir_all(&self.network_root)?;

        // Port descriptor
        create_dir_all(&self.port_descriptor_dir)?;

        Ok(())
    }

    pub async fn load_network_descriptor(
        &self,
    ) -> Result<Option<NetworkDescriptorModel>, LoadNetworkFileError> {
        self.root()?
            .with_read(async |root| {
                json::load(&root.network_descriptor_path()).or_else(|err| match err {
                    // Default to empty
                    json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(None),

                    // Other
                    _ => Err(err.into()),
                })
            })
            .await?
    }

    pub async fn load_port_descriptor(
        &self,
        port: u16,
    ) -> Result<Option<NetworkDescriptorModel>, LoadNetworkFileError> {
        self.port(port)?
            .with_read(async |paths| {
                json::load(&paths.descriptor_path()).or_else(|err| match err {
                    // Default to empty
                    json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(None),

                    // Other
                    _ => Err(err.into()),
                })
            })
            .await?
    }

    pub async fn cleanup_project_network_descriptor(
        &self,
    ) -> Result<(), CleanupNetworkDescriptorError> {
        self.root()?
            .with_write(async |root| crate::fs::remove_file(&root.network_descriptor_path()))
            .await??;
        Ok(())
    }

    pub async fn cleanup_port_descriptor(
        &self,
        gateway_port: Option<u16>,
    ) -> Result<(), CleanupNetworkDescriptorError> {
        if let Some(port) = gateway_port {
            self.port(port)?
                .with_write(async |paths| crate::fs::remove_file(&paths.descriptor_path()))
                .await??;
        }
        Ok(())
    }

    pub async fn save_background_network_runner_pid(&self, pid: Pid) -> Result<(), SavePidError> {
        self.root()?
            .with_write(async |root| {
                crate::fs::write(
                    &root.background_network_runner_pid_file(),
                    format!("{pid}").as_bytes(),
                )?;
                Ok(())
            })
            .await?
    }

    pub async fn load_background_network_runner_pid(&self) -> Result<Option<Pid>, LoadPidError> {
        self.root()?
            .with_read(async |root| {
                let path = root.background_network_runner_pid_file();

                read_to_string(&path)
                    .map(|content| content.trim().parse::<Pid>().ok())
                    .or_else(|err| match err.kind() {
                        ErrorKind::NotFound => Ok(None),
                        _ => Err(err).context(ReadPidSnafu { path: path.clone() }),
                    })
            })
            .await?
    }

    pub fn root(&self) -> Result<DirectoryStructureLock<NetworkRootPaths>, LockError> {
        DirectoryStructureLock::open_or_create(NetworkRootPaths {
            network_root: self.network_root.clone(),
        })
    }

    pub fn port(&self, port: u16) -> Result<DirectoryStructureLock<PortPaths>, LockError> {
        DirectoryStructureLock::open_or_create(PortPaths {
            port_descriptor_dir: self.port_descriptor_dir.clone(),
            port,
        })
    }
}

pub async fn save_network_descriptors(
    root: LWrite<&NetworkRootPaths>,
    port: Option<LWrite<&PortPaths>>,
    descriptor: &NetworkDescriptorModel,
) -> Result<(), SaveNetworkDescriptorError> {
    if let Some(port) = port {
        json::save(&port.descriptor_path(), descriptor).context(PortDescriptorSnafu)?;
    }
    json::save(&root.network_descriptor_path(), descriptor).context(NetworkRootDescriptorSnafu)?;
    Ok(())
}

pub struct NetworkRootPaths {
    network_root: PathBuf,
}

impl NetworkRootPaths {
    pub fn root_dir(&self) -> &Path {
        &self.network_root
    }

    pub fn network_descriptor_path(&self) -> PathBuf {
        self.network_root.join("descriptor.json")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.network_root.join("state")
    }

    pub fn pocketic_dir(&self) -> PathBuf {
        self.network_root.join("pocket-ic")
    }

    /// When running a network in the background, we store the PID of the background controlling `icp` process here.
    /// This does _not_ contain pocket-ic processess.
    pub fn background_network_runner_pid_file(&self) -> PathBuf {
        self.network_root.join("background_network_runner.pid")
    }

    // pocketic expects this file not to exist when launching it.
    // pocketic populates it with the port number, and deletes the file when it exits.
    // if the file exists, pocketic assumes this means another pocketic instance
    // is running, and exits with exit code(0).
    pub fn pocketic_port_file(&self) -> PathBuf {
        self.pocketic_dir().join("port")
    }
}

impl PathsAccess for NetworkRootPaths {
    fn lock_file(&self) -> PathBuf {
        self.network_root.join("lock")
    }
}

pub struct PortPaths {
    port_descriptor_dir: PathBuf,
    port: u16,
}

impl PathsAccess for PortPaths {
    fn lock_file(&self) -> PathBuf {
        self.port_descriptor_dir.join(format!("{}.lock", self.port))
    }
}

impl PortPaths {
    pub fn descriptor_path(&self) -> PathBuf {
        self.port_descriptor_dir.join(format!("{}.json", self.port))
    }
}

#[derive(Debug, Snafu)]
pub enum SaveNetworkDescriptorError {
    #[snafu(display("Failed to write port descriptor"))]
    PortDescriptor { source: json::Error },
    #[snafu(display("Failed to write network descriptor"))]
    NetworkRootDescriptor { source: json::Error },
}

#[derive(Debug, Snafu)]
pub enum CleanupNetworkDescriptorError {
    #[snafu(transparent)]
    LockFileError { source: LockError },
    #[snafu(transparent)]
    DeleteFileError { source: crate::fs::Error },
}

#[derive(Debug, Snafu)]
pub enum SavePidError {
    #[snafu(transparent)]
    LockFileError { source: LockError },

    #[snafu(transparent)]
    WritePid { source: crate::fs::Error },
}

#[derive(Debug, Snafu)]
pub enum LoadPidError {
    #[snafu(display("failed to read PID from {path}"))]
    ReadPid {
        source: crate::fs::Error,
        path: PathBuf,
    },
    #[snafu(transparent)]
    LockFileError { source: LockError },
}
