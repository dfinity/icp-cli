use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use snafu::prelude::*;

pub use directory::{LoadPidError, NetworkDirectory, SavePidError};
pub use managed::run::{RunNetworkError, run_network};
use strum::EnumString;
use url::Url;

use crate::{
    CACHE_DIR, ICP_BASE, Network,
    manifest::{
        ProjectRootLocate, ProjectRootLocateError,
        network::{Connected as ManifestConnected, Endpoints, Gateway as ManifestGateway, Mode},
    },
    network::access::{
        GetNetworkAccessError, NetworkAccess, get_connected_network_access,
        get_managed_network_access,
    },
    prelude::*,
};

pub mod access;
pub mod config;
pub mod directory;
pub mod managed;

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize)]
pub enum Port {
    Fixed(u16),
    Random,
}

impl Default for Port {
    fn default() -> Self {
        Port::Fixed(8000)
    }
}

impl<'de> Deserialize<'de> for Port {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match u16::deserialize(d)? {
            0 => Port::Random,
            p => Port::Fixed(p),
        })
    }
}

fn default_host() -> String {
    "localhost".to_string()
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Gateway {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default)]
    pub port: Port,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Managed {
    #[serde(flatten)]
    pub mode: ManagedMode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(untagged)]
pub enum ManagedMode {
    Image(Box<ManagedImageConfig>),
    Launcher(Box<ManagedLauncherConfig>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct ManagedLauncherConfig {
    pub gateway: Gateway,
    pub artificial_delay_ms: Option<u64>,
    pub ii: bool,
    pub nns: bool,
    pub subnets: Option<Vec<SubnetKind>>,
    pub version: Option<String>,
}

#[derive(
    Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize, EnumString, strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SubnetKind {
    Application,
    System,
    VerifiedApplication,
    Bitcoin,
    Fiduciary,
    Nns,
    Sns,
}

impl Default for ManagedMode {
    fn default() -> Self {
        Self::default_for_port(0)
    }
}

impl ManagedMode {
    pub fn default_for_port(port: u16) -> Self {
        ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
            gateway: Gateway {
                host: default_host(),
                port: if port == 0 {
                    Port::Random
                } else {
                    Port::Fixed(port)
                },
            },
            artificial_delay_ms: None,
            ii: false,
            nns: false,
            subnets: None,
            version: None,
        }))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct ManagedImageConfig {
    pub image: String,
    pub port_mapping: Vec<String>,
    pub rm_on_exit: bool,
    pub args: Vec<String>,
    pub entrypoint: Option<Vec<String>>,
    pub environment: Vec<String>,
    pub volumes: Vec<String>,
    pub platform: Option<String>,
    pub user: Option<String>,
    pub shm_size: Option<i64>,
    pub status_dir: String,
    pub mounts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    /// The URL this network's API can be reached at.
    pub api_url: Url,

    /// The URL this network's HTTP gateway can be reached at.
    pub gateway_url: Option<Url>,

    /// The root key of this network
    pub root_key: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Configuration {
    // Note: we must use struct variants to be able to flatten
    // and make schemars generate the proper schema
    /// A managed network is one which can be controlled and manipulated.
    Managed {
        #[serde(flatten)]
        managed: Managed,
    },

    /// A connected network is one which can be interacted with
    /// but cannot be controlled or manipulated.
    Connected {
        #[serde(flatten)]
        connected: Connected,
    },
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration::Managed {
            managed: Managed::default(),
        }
    }
}

impl From<ManifestGateway> for Gateway {
    fn from(value: ManifestGateway) -> Self {
        let host = value.host.unwrap_or("localhost".to_string());
        let port = match value.port {
            Some(0) => Port::Random,
            Some(p) => Port::Fixed(p),
            None => Port::Random,
        };
        Gateway { host, port }
    }
}

impl From<ManifestConnected> for Connected {
    fn from(value: ManifestConnected) -> Self {
        match value.endpoints {
            Endpoints::Implicit { url } => Connected {
                api_url: url.clone(),
                gateway_url: Some(url),
                root_key: value.root_key.map(|rk| rk.0),
            },
            Endpoints::Explicit {
                api_url,
                gateway_url,
            } => Connected {
                api_url,
                gateway_url,
                root_key: value.root_key.map(|rk| rk.0),
            },
        }
    }
}

impl From<Mode> for Configuration {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Managed(managed) => match *managed.mode {
                crate::manifest::network::ManagedMode::Launcher {
                    gateway,
                    artificial_delay_ms,
                    ii,
                    nns,
                    subnets,
                    version,
                } => {
                    let gateway: Gateway = match gateway {
                        Some(g) => g.into(),
                        None => Gateway::default(),
                    };
                    let version = match version {
                        Some(v) => {
                            if v.starts_with('v') {
                                Some(v)
                            } else {
                                Some(format!("v{v}"))
                            }
                        }
                        None => None,
                    };
                    Configuration::Managed {
                        managed: Managed {
                            mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                                gateway,
                                artificial_delay_ms,
                                ii: ii.unwrap_or(false),
                                nns: nns.unwrap_or(false),
                                subnets,
                                version,
                            })),
                        },
                    }
                }
                crate::manifest::network::ManagedMode::Image {
                    image,
                    port_mapping,
                    rm_on_exit,
                    args,
                    entrypoint,
                    environment,
                    volumes,
                    platform,
                    user,
                    shm_size,
                    status_dir,
                    mounts: mount,
                } => Configuration::Managed {
                    managed: Managed {
                        mode: ManagedMode::Image(Box::new(ManagedImageConfig {
                            image,
                            port_mapping,
                            rm_on_exit: rm_on_exit.unwrap_or(false),
                            args: args.unwrap_or_default(),
                            entrypoint,
                            environment: environment.unwrap_or_default(),
                            volumes: volumes.unwrap_or_default(),
                            platform,
                            user,
                            shm_size,
                            status_dir: status_dir.unwrap_or_else(|| "/app/status".to_string()),
                            mounts: mount.unwrap_or_default(),
                        })),
                    },
                },
            },
            Mode::Connected(connected) => Configuration::Connected {
                connected: connected.into(),
            },
        }
    }
}

#[derive(Debug, Snafu)]
pub enum AccessError {
    #[snafu(display("failed to find project root"))]
    ProjectRootLocate { source: ProjectRootLocateError },

    #[snafu(transparent)]
    GetNetworkAccess { source: GetNetworkAccessError },
}

#[async_trait]
pub trait Access: Sync + Send {
    fn get_network_directory(&self, network: &Network) -> Result<NetworkDirectory, AccessError>;
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError>;
}

pub struct Accessor {
    // Project root
    pub project_root_locate: Arc<dyn ProjectRootLocate>,

    // Port descriptors dir
    pub descriptors: PathBuf,
}

#[async_trait]
impl Access for Accessor {
    /// The network directory is located at `<project_root>/.icp/cache/networks/<network_name>`.
    fn get_network_directory(&self, network: &Network) -> Result<NetworkDirectory, AccessError> {
        let dir = self
            .project_root_locate
            .locate()
            .context(ProjectRootLocateSnafu)?;
        Ok(NetworkDirectory::new(
            &network.name,
            &dir.join(ICP_BASE)
                .join(CACHE_DIR)
                .join("networks")
                .join(&network.name),
            &self.descriptors,
        ))
    }
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        match &network.configuration {
            Configuration::Managed { managed: _ } => {
                let nd = self.get_network_directory(network)?;
                Ok(get_managed_network_access(nd).await?)
            }
            Configuration::Connected { connected: cfg } => {
                Ok(get_connected_network_access(cfg).await?)
            }
        }
    }
}

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
pub struct MockNetworkAccessor {
    /// Network-specific access configurations by network name
    networks: HashMap<String, NetworkAccess>,
}

#[cfg(test)]
impl MockNetworkAccessor {
    /// Creates a new empty mock network accessor.
    pub fn new() -> Self {
        Self {
            networks: HashMap::new(),
        }
    }

    /// Adds a network-specific access configuration.
    pub fn with_network(mut self, name: impl Into<String>, access: NetworkAccess) -> Self {
        self.networks.insert(name.into(), access);
        self
    }
}

#[cfg(test)]
impl Default for MockNetworkAccessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[async_trait]
impl Access for MockNetworkAccessor {
    fn get_network_directory(&self, network: &Network) -> Result<NetworkDirectory, AccessError> {
        Ok(NetworkDirectory {
            network_name: network.name.clone(),
            network_root: PathBuf::new(),
            port_descriptor_dir: PathBuf::new(),
        })
    }
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        self.networks
            .get(&network.name)
            .cloned()
            .ok_or_else(|| AccessError::GetNetworkAccess {
                source: GetNetworkAccessError::NetworkNotRunning {
                    network: network.name.clone(),
                },
            })
    }
}
