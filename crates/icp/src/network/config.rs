//! Network descriptor types for persisting managed network state.
//!
//! A **network descriptor** is a JSON file that captures the runtime state of a running
//! managed network. It serves several purposes:
//!
//! 1. **Process tracking**: Stores the PID (or container ID) so the network can be stopped later
//! 2. **Liveness detection**: Includes process start time to detect PID reuse after system reboot
//! 3. **Connection info**: Stores the gateway port and root key needed to connect an IC agent
//! 4. **Port reservation**: For fixed ports, a copy in the global directory prevents conflicts
//!
//! See [`crate::network::directory`] for the file hierarchy where descriptors are stored.

use candid::Principal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::prelude::*;

/// Gateway port configuration within a network descriptor.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorGatewayPort {
    /// If true, this is a user-specified fixed port and a global port descriptor exists.
    /// If false, the port was randomly assigned and no global descriptor is written.
    pub fixed: bool,
    /// The TCP port the gateway is listening on.
    pub port: u16,
}

/// Runtime state of a running managed network, persisted as `descriptor.json`.
///
/// This is written when a network starts and read when connecting to or stopping the network.
/// The descriptor uniquely identifies the network instance via [`Self::id`] and tracks
/// the process/container via [`Self::child_locator`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorModel {
    /// Schema version, currently `"1"`.
    pub v: String,
    /// Unique identifier for this network instance. Used to correlate project-local
    /// and global port descriptors.
    pub id: Uuid,
    /// The project directory that owns this network.
    pub project_dir: PathBuf,
    /// The network name (e.g., "local").
    pub network: String,
    /// The project-local network directory where this descriptor is stored.
    pub network_dir: PathBuf,
    /// Gateway port configuration.
    pub gateway: NetworkDescriptorGatewayPort,
    /// Locator for the network process or container.
    pub child_locator: ChildLocator,
    /// The network's root key.
    #[serde(with = "hex::serde")]
    pub root_key: Vec<u8>,
    /// PocketIC configuration API port (launcher mode only).
    pub pocketic_config_port: Option<u16>,
    /// PocketIC instance ID within the launcher (launcher mode only).
    pub pocketic_instance_id: Option<usize>,
    /// Canister ID of the deployed Candid UI, if any.
    pub candid_ui_canister_id: Option<Principal>,
    /// Canister ID of the deployed proxy canister, if any.
    pub proxy_canister_id: Option<Principal>,
}

/// Identifies the process or container running a managed network.
///
/// Used to check if the network is still alive and to stop it when requested.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum ChildLocator {
    /// A native process (used on macOS/Linux with the network launcher).
    Pid {
        pid: u32,
        /// Process start time in seconds since UNIX epoch. Used to detect PID reuse:
        /// if the PID exists but has a different start time, the original process died.
        #[serde(default)] // compat with existing descriptors
        start_time: u64,
    },
    /// A Docker container (used on Windows or when explicitly configured).
    Container {
        /// Docker container ID.
        id: String,
        /// Docker socket path (e.g., `/var/run/docker.sock`).
        socket: String,
        /// Whether to remove the container when it exits.
        rm_on_exit: bool,
    },
}

impl ChildLocator {
    /// Checks if the process or container referenced by this locator is still alive.
    pub async fn is_alive(&self) -> bool {
        match self {
            ChildLocator::Pid { pid, start_time } => {
                use sysinfo::{Pid, ProcessesToUpdate, System};
                let mut system = System::new();
                let sysinfo_pid = Pid::from_u32(*pid);
                system.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), true);
                system
                    .process(sysinfo_pid)
                    .is_some_and(|p| p.start_time() == *start_time)
            }
            ChildLocator::Container { id, socket, .. } => {
                crate::network::managed::docker::is_container_running(socket, id).await
            }
        }
    }
}

impl NetworkDescriptorModel {
    pub fn gateway_port(&self) -> Option<u16> {
        if self.gateway.fixed {
            return Some(self.gateway.port);
        }

        None
    }
}
