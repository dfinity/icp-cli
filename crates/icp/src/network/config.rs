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

use std::time::Duration;

use candid::Principal;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;
use url::Url;
use uuid::Uuid;

use crate::prelude::*;

/// How long to wait for the gateway to answer before concluding the network is defunct.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

fn default_gateway_host() -> String {
    "localhost".to_string()
}

fn default_gateway_ip() -> String {
    "127.0.0.1".to_string()
}

/// Gateway port configuration within a network descriptor.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorGatewayPort {
    /// If true, this is a user-specified fixed port and a global port descriptor exists.
    /// If false, the port was randomly assigned and no global descriptor is written.
    pub fixed: bool,
    /// The TCP port the gateway is listening on.
    pub port: u16,
    /// The host to use when constructing URLs to reach the gateway.
    #[serde(default = "default_gateway_host")]
    pub host: String,
    /// The IP address to use when constructing URLs to reach the API.
    #[serde(default = "default_gateway_ip")]
    pub ip: String,
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
    /// Whether Internet Identity is deployed on this network.
    #[serde(default)]
    pub ii: bool,
    /// Path to the status directory shared with the network launcher.
    /// Used to write `custom-domains.txt` for friendly domain routing.
    #[serde(default)]
    pub status_dir: Option<PathBuf>,
    /// Whether the network supports friendly domain routing (e.g., `foo.local.localhost`).
    #[serde(default)]
    pub use_friendly_domains: bool,
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

    /// Asks the gateway whether it is serving requests.
    ///
    /// A launcher can outlive the replica it supervises — a host suspend can leave the process
    /// running with a defunct PocketIC behind it — so [`ChildLocator::is_alive`] is not on its
    /// own evidence that the network works.
    ///
    /// `Ok(false)` means the network answered for itself that it is unusable: nothing accepted
    /// the connection, it never replied, or it replied with a failure. Any other outcome is an
    /// error, because it leaves the question unanswered rather than answering it in the
    /// negative.
    pub async fn is_responsive(&self) -> Result<bool, ProbeNetworkError> {
        let status_url = Url::parse(&format!(
            "http://{}:{}/api/v2/status",
            self.gateway.host, self.gateway.port
        ))
        .context(ParseStatusUrlSnafu {
            host: &self.gateway.host,
            port: self.gateway.port,
        })?;
        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .context(BuildProbeClientSnafu {
                url: status_url.clone(),
            })?;
        match client.get(status_url.clone()).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(source) if source.is_connect() || source.is_timeout() => Ok(false),
            Err(source) => Err(ProbeNetworkError::QueryStatus {
                source,
                url: status_url,
            }),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum ProbeNetworkError {
    #[snafu(display("failed to build a status URL for the gateway at {host}:{port}"))]
    ParseStatusUrl {
        source: url::ParseError,
        host: String,
        port: u16,
    },
    #[snafu(display("failed to build an HTTP client to probe {url}"))]
    BuildProbeClient { source: reqwest::Error, url: Url },
    #[snafu(display("failed to query the network status at {url}"))]
    QueryStatus { source: reqwest::Error, url: Url },
}
