//! Network directory management for managed networks.
//!
//! This module handles the file hierarchy for storing network runtime state. There are two
//! directory hierarchies involved:
//!
//! # Project-Local Network Directory
//!
//! Each project stores its managed network state under `.icp/cache/networks/<network-name>/`:
//!
//! ```text
//! .icp/
//! └── cache/
//!     └── networks/
//!         └── <network-name>/
//!             ├── descriptor.json      # Network descriptor (runtime state)
//!             ├── lock                 # File lock for concurrent access
//!             ├── state/               # PocketIC state directory
//!             └── network-launcher/
//!                 ├── stdout.log
//!                 └── stderr.log
//! ```
//!
//! The `descriptor.json` file is a [`NetworkDescriptorModel`] that captures the running network's
//! state: process ID, gateway port, root key, etc.
//!
//! # Global Port Descriptor Directory
//!
//! When a managed network uses a **fixed port** (e.g., the default `local` network on port 8000),
//! a port descriptor is also written to a global directory to prevent port conflicts across
//! projects. The global directory location depends on the platform:
//!
//! - **Windows**: `~\AppData\Local\icp-cli\cache\port-descriptors\`
//! - **macOS**: `~/Library/Caches/org.dfinity.icp-cli/port-descriptors/`
//! - **Linux**: `~/.cache/icp-cli/port-descriptors/`
//! - **With `ICP_HOME`**: `$ICP_HOME/port-descriptors/`
//!
//! ```text
//! <global-cache>/
//! └── port-descriptors/
//!     ├── 8000.json    # Descriptor for port 8000
//!     ├── 8000.lock    # Lock file for port 8000
//!     └── ...
//! ```
//!
//! When starting a network on a fixed port, icp-cli:
//! 1. Checks if `<port>.json` exists and references a live process
//! 2. If so, returns an error indicating the port is in use by another project
//! 3. If not (or the process is dead), claims the port by writing a new descriptor
//!
//! Networks using random ports (non-fixed) do not write to the global directory.

use std::io::ErrorKind;

use snafu::{ResultExt, prelude::*};

use crate::{
    fs::{
        create_dir_all, json,
        lock::{DirectoryStructureLock, LWrite, LockError, PathsAccess},
    },
    network::config::NetworkDescriptorModel,
    prelude::*,
};

/// Interface to network data directories for a single managed network.
///
/// This struct provides access to both:
/// - The **project-local** network directory (`.icp/cache/networks/<name>/`)
/// - The **global** port descriptor directory (for fixed-port networks)
///
/// All file operations are protected by file locks to handle concurrent access
/// from multiple CLI invocations.
#[derive(Clone)]
pub struct NetworkDirectory {
    /// The name of the network (e.g., "local").
    pub network_name: String,
    /// Root directory for this network's project-local state (descriptor, logs, PocketIC state).
    pub network_root: PathBuf,
    /// Global directory for port descriptors, shared across all projects.
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

/// Saves the network descriptor to both the project-local directory and the global port
/// descriptor directory (if a fixed port is used).
///
/// This must be called with write locks already acquired on both directories.
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

/// Path accessors for the project-local network directory.
///
/// Provides paths to:
/// - `descriptor.json` - The [`NetworkDescriptorModel`] capturing runtime state
/// - `state/` - PocketIC's state directory (canister data, checkpoints)
/// - `network-launcher/` - Launcher process logs
///
/// All access should go through [`DirectoryStructureLock`] to ensure proper locking.
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

/// Path accessors for a port descriptor in the global directory.
///
/// Each fixed port has two files:
/// - `<port>.json` - The [`NetworkDescriptorModel`] of the network using this port
/// - `<port>.lock` - File lock for concurrent access
///
/// These files are used to detect when multiple projects try to use the same port.
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

    /// Checks if the port is in use by a live process/container.
    /// Returns the descriptor if the port is in use, None if available.
    pub async fn check_port_in_use(
        &self,
    ) -> Result<Option<NetworkDescriptorModel>, CheckPortInUseError> {
        match json::load::<NetworkDescriptorModel>(&self.descriptor_path()) {
            Ok(descriptor) => {
                if descriptor.child_locator.is_alive().await {
                    Ok(Some(descriptor))
                } else {
                    // Process/container is dead, port is available
                    Ok(None)
                }
            }
            Err(json::Error::Io { source }) if source.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CheckPortInUseError::LoadDescriptor { source: e }),
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
pub enum CheckPortInUseError {
    #[snafu(display("failed to load port descriptor"))]
    LoadDescriptor { source: json::Error },
}

#[derive(Debug, Snafu)]
#[snafu(display("port {port} is in use by the {network} network of the project at '{owner}'"))]
pub struct PortInUseError {
    pub port: u16,
    pub network: String,
    pub owner: PathBuf,
}
