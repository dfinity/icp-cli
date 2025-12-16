use std::{
    fs::{File, TryLockError},
    io::ErrorKind,
};

use snafu::{ResultExt, prelude::*};

use crate::{
    fs::{
        create_dir_all, json,
        lock::{DirectoryStructureLock, LWrite, LockError, PathsAccess},
    },
    network::config::NetworkDescriptorModel,
    prelude::*,
};

/// General interface to network data directories for a single network, covering both the local network root
/// and the port descriptor in the global directory.
#[derive(Clone)]
pub struct NetworkDirectory {
    pub network_name: String,
    /// Project-local network data directory
    pub network_root: PathBuf,
    /// Global fixed-port descriptor directory
    pub port_descriptor_dir: PathBuf,
}

impl NetworkDirectory {
    pub(crate) fn new(network_name: &str, network_root: &Path, port_descriptor_dir: &Path) -> Self {
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
    pub fn ensure_exists(&self) -> Result<(), crate::fs::IoError> {
        // Network root
        create_dir_all(&self.network_root)?;

        // Port descriptor
        create_dir_all(&self.port_descriptor_dir)?;

        Ok(())
    }

    /// Reads the descriptor from the local network root. Returns None if it does not exist.
    pub async fn load_network_descriptor(
        &self,
    ) -> Result<Option<NetworkDescriptorModel>, LoadNetworkFileError> {
        self.root()?
            .with_read(async |root| {
                json::load(&root.network_descriptor_path()).or_else(|err| match err {
                    // Default to empty
                    json::Error::Io { source } if source.kind() == ErrorKind::NotFound => Ok(None),

                    // Other
                    _ => Err(err.into()),
                })
            })
            .await?
    }

    /// Reads the descriptor for the given port. Returns None if it does not exist.
    pub async fn load_port_descriptor(
        &self,
        port: u16,
    ) -> Result<Option<NetworkDescriptorModel>, LoadNetworkFileError> {
        self.port(port)?
            .with_read(async |paths| {
                json::load(&paths.descriptor_path()).or_else(|err| match err {
                    // Default to empty
                    json::Error::Io { source } if source.kind() == ErrorKind::NotFound => Ok(None),

                    // Other
                    _ => Err(err.into()),
                })
            })
            .await?
    }

    /// Deletes the network descriptor in the local network root.
    pub async fn cleanup_project_network_descriptor(
        &self,
    ) -> Result<(), CleanupNetworkDescriptorError> {
        self.root()?
            .with_write(async |root| crate::fs::remove_file(&root.network_descriptor_path()))
            .await??;
        Ok(())
    }

    /// Deletes the port descriptor from the global descriptor directory.
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

    /// Locked directory access for the local network root.
    pub fn root(&self) -> Result<DirectoryStructureLock<NetworkRootPaths>, LockError> {
        DirectoryStructureLock::open_or_create(NetworkRootPaths {
            network_root: self.network_root.clone(),
        })
    }

    /// Locked directory access for a port descriptor.
    pub fn port(&self, port: u16) -> Result<DirectoryStructureLock<PortPaths>, LockError> {
        DirectoryStructureLock::open_or_create(PortPaths {
            port_descriptor_dir: self.port_descriptor_dir.clone(),
            port,
        })
    }
}

/// Saves the network descriptor to the both local network root and port descriptor (if provided).
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

/// Directory structure for the local network root.
pub struct NetworkRootPaths {
    network_root: PathBuf,
}

impl NetworkRootPaths {
    /// The root directory of the network, usually `./.icp/networks/<network-name>/`
    pub fn root_dir(&self) -> &Path {
        &self.network_root
    }

    /// The path to the network descriptor file
    pub fn network_descriptor_path(&self) -> PathBuf {
        self.network_root.join("descriptor.json")
    }

    /// The path to the state directory. Be careful that the network is not running before attempting
    /// to read or write this location.
    pub fn state_dir(&self) -> PathBuf {
        self.network_root.join("state")
    }

    /// Subdirectory for network-launcher-related files (but not the state directory)
    pub fn launcher_dir(&self) -> PathBuf {
        self.network_root.join("network-launcher")
    }

    /// icp-cli may write the network's stdout to this file.
    pub fn network_stdout_file(&self) -> PathBuf {
        self.launcher_dir().join("stdout.log")
    }

    /// icp-cli may write the network's stderr to this file.
    pub fn network_stderr_file(&self) -> PathBuf {
        self.launcher_dir().join("stderr.log")
    }
}

impl PathsAccess for NetworkRootPaths {
    fn lock_file(&self) -> PathBuf {
        self.network_root.join("lock")
    }
}

/// Represents the lock
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
    /// Path to the port descriptor file
    pub fn descriptor_path(&self) -> PathBuf {
        self.port_descriptor_dir.join(format!("{}.json", self.port))
    }

    /// Claims ownership of a port for this process while the returned file handle is active. Does not impact
    /// file locking.
    ///
    /// Returns `PortAlreadyClaimed` if another process has already claimed the port.
    pub fn claim_port(&self) -> Result<File, ClaimPortError> {
        let claim_path = self.descriptor_path().with_extension("claim");
        let f = File::create(&claim_path).context(OpenClaimFileSnafu { path: claim_path })?;
        if let Err(e) = f.try_lock() {
            match e {
                TryLockError::WouldBlock => {
                    if let Ok(descriptor) =
                        json::load::<NetworkDescriptorModel>(&self.descriptor_path())
                    {
                        Err(ClaimPortError::PortAlreadyClaimed {
                            port: self.port,
                            network: Some(descriptor.network),
                            owner: Some(descriptor.project_dir),
                        })
                    } else {
                        Err(ClaimPortError::PortAlreadyClaimed {
                            port: self.port,
                            network: None,
                            owner: None,
                        })
                    }
                }
                TryLockError::Error(err) => Err(ClaimPortError::LockErrorOther {
                    port: self.port,
                    source: err,
                }),
            }
        } else {
            Ok(f)
        }
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
    DeleteFileError { source: crate::fs::IoError },
}

#[derive(Debug, Snafu)]
pub enum SavePidError {
    #[snafu(transparent)]
    LockFileError { source: LockError },

    #[snafu(transparent)]
    WritePid { source: crate::fs::IoError },
}

#[derive(Debug, Snafu)]
pub enum LoadPidError {
    #[snafu(display("failed to read PID from {path}"))]
    ReadPid {
        source: crate::fs::IoError,
        path: PathBuf,
    },
    #[snafu(transparent)]
    LockFileError { source: LockError },
}

#[derive(Debug, Snafu)]
pub enum ClaimPortError {
    #[snafu(transparent)]
    OpenPortLockError { source: LockError },

    #[snafu(display("failed to open port claim file {path}"))]
    OpenClaimFileError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("port {port} is in use by the {} network of the project at '{}'",
        network.as_ref().map_or_else(|| "<unknown>", |n| n.as_str()),
        owner.as_ref().map_or_else(|| "<unknown>", |p| p.as_str()),
    ))]
    PortAlreadyClaimed {
        port: u16,
        network: Option<String>,
        owner: Option<PathBuf>,
    },

    #[snafu(display("failed to claim port {port}"))]
    LockErrorOther { port: u16, source: std::io::Error },
}
