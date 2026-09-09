use async_trait::async_trait;
use candid::Principal;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use snafu::prelude::*;

pub use crate::manifest::network::RootKeySpec;
pub use access::{NetworkAccess, NetworkUrls, RootKeySource};
use strum::EnumString;
use url::Url;

use crate::{
    Network,
    manifest::network::{
        Connected as ManifestConnected, Endpoints, Gateway as ManifestGateway, Mode,
    },
    project::DEFAULT_LOCAL_NETWORK_PORT,
};

pub mod access;

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

fn default_bind() -> String {
    "127.0.0.1".to_string()
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Gateway {
    #[serde(default = "default_bind")]
    pub bind: String,

    #[serde(default)]
    pub port: Port,

    #[serde(default)]
    pub domains: Vec<String>,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: Default::default(),
            domains: Default::default(),
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
    pub bitcoind_addr: Option<Vec<String>>,
    pub dogecoind_addr: Option<Vec<String>>,
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
        Self::default_for_port(DEFAULT_LOCAL_NETWORK_PORT)
    }
}

impl ManagedMode {
    pub fn default_for_port(port: u16) -> Self {
        ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
            gateway: Gateway {
                bind: default_bind(),
                port: if port == 0 {
                    Port::Random
                } else {
                    Port::Fixed(port)
                },
                domains: vec![],
            },
            artificial_delay_ms: None,
            ii: false,
            nns: false,
            subnets: None,
            bitcoind_addr: None,
            dogecoind_addr: None,
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
    pub extra_hosts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    /// The URL this network's API can be reached at.
    pub api_url: Url,

    /// The URL this network's HTTP gateway can be reached at.
    pub http_gateway_url: Option<Url>,

    /// How to obtain the root key used to verify responses from this network.
    pub root_key: RootKeySpec,
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
        let ManifestGateway {
            bind,
            domains,
            port,
        } = value;
        let bind = bind.unwrap_or("127.0.0.1".to_string());
        let port = match port {
            Some(0) => Port::Random,
            Some(p) => Port::Fixed(p),
            None => Port::default(),
        };
        let mut domains = domains.unwrap_or_default();
        if bind == "127.0.0.1" || bind == "0.0.0.0" || bind == "::1" || bind == "::" {
            domains.insert(0, "localhost".to_string());
        }
        Gateway {
            bind,
            port,
            domains,
        }
    }
}

impl From<ManifestConnected> for Connected {
    fn from(value: ManifestConnected) -> Self {
        let root_key = value.root_key;
        match value.endpoints {
            Endpoints::Implicit { url } => Connected {
                api_url: url.clone(),
                http_gateway_url: Some(url),
                root_key,
            },
            Endpoints::Explicit {
                api_url,
                http_gateway_url,
            } => Connected {
                api_url,
                http_gateway_url,
                root_key,
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
                    bitcoind_addr,
                    dogecoind_addr,
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
                                bitcoind_addr,
                                dogecoind_addr,
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
                    extra_hosts,
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
                            extra_hosts: extra_hosts.unwrap_or_default(),
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

/// A network could not be reached, or could not be described.
///
/// What that took is the implementation's business: locating the project,
/// reading a descriptor some launcher wrote, fetching a root key over HTTP.
/// This layer knows only that it can fail and that whatever went wrong is what
/// the user needs to be told, so the cause is carried whole and displayed as
/// itself rather than being restated here.
#[derive(Debug, Snafu)]
#[snafu(display("{source}"))]
pub struct AccessError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl AccessError {
    /// Wraps an implementation's own error for the trait boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

/// One environment's friendly-name mappings, as collected from the project.
#[derive(Clone, Debug)]
pub struct FriendlyDomains {
    /// Environment the mappings belong to.
    pub environment: String,

    /// `(friendly name, canister id)`, one entry per friendly name — so several
    /// for a de-duplicated shared dependency canister.
    pub entries: Vec<(String, Principal)>,
}

/// Collects the mappings of every environment that targets the named network.
///
/// The project layer knows which canisters have ids and what they are called;
/// which network is actually being served, and where the mapping is written, is
/// this layer's business. So the project hands over the means to collect rather
/// than a finished collection, and [`Access::publish_friendly_domains`] names
/// the network — and decides whether to ask at all.
pub type CollectFriendlyDomains<'a> = dyn Fn(&str) -> Vec<FriendlyDomains> + Send + Sync + 'a;

#[async_trait]
pub trait Access: Sync + Send {
    /// The network's endpoints together with the trust material needed to
    /// verify what it says.
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError>;

    /// The network's URLs alone. Unlike [`Access::access`] this resolves no root
    /// key, so a caller that only needs an endpoint does not make a connected
    /// network fetch one.
    async fn urls(&self, network: &Network) -> Result<NetworkUrls, AccessError>;

    /// Rewrites the friendly-domain mapping a running managed network serves.
    ///
    /// Best-effort by contract: a network that is not running, or is not
    /// managed, or whose mapping cannot be written, is not an error — this is
    /// called on paths (canister creation, deletion) that must not fail because
    /// a convenience URL is stale.
    ///
    /// Call `collect` only once it is settled that there is something to
    /// publish, and only for the network being served: reading the mappings
    /// costs an id-store read per environment, and a stopped network should
    /// cost none.
    async fn publish_friendly_domains(
        &self,
        network: &Network,
        collect: &CollectFriendlyDomains<'_>,
    );
}

#[cfg(any(test, feature = "test-util"))]
use std::collections::HashMap;

/// A [`MockNetworkAccessor`] was asked about a network it was not given.
#[cfg(any(test, feature = "test-util"))]
#[derive(Debug, Snafu)]
#[snafu(display("the {network} network for this project is not running"))]
pub struct NotConfigured {
    pub network: String,
}

#[cfg(any(test, feature = "test-util"))]
pub struct MockNetworkAccessor {
    /// Network-specific access configurations by network name
    networks: HashMap<String, NetworkAccess>,
}

#[cfg(any(test, feature = "test-util"))]
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

#[cfg(any(test, feature = "test-util"))]
impl Default for MockNetworkAccessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl Access for MockNetworkAccessor {
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        self.networks.get(&network.name).cloned().ok_or_else(|| {
            AccessError::new(NotConfigured {
                network: network.name.clone(),
            })
        })
    }

    async fn urls(&self, network: &Network) -> Result<NetworkUrls, AccessError> {
        let access = self.access(network).await?;
        Ok(NetworkUrls {
            api_url: access.api_url,
            http_gateway_url: access.http_gateway_url,
        })
    }

    /// The mock serves no friendly domains, so it never asks for any.
    async fn publish_friendly_domains(
        &self,
        _network: &Network,
        _collect: &CollectFriendlyDomains<'_>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::network::{
        Gateway as ManifestGateway, Managed as ManifestManaged, ManagedMode as ManifestManagedMode,
        Mode,
    };

    #[test]
    fn from_mode_launcher_with_bitcoind_addr() {
        let mode = Mode::Managed(ManifestManaged {
            mode: Box::new(ManifestManagedMode::Launcher {
                gateway: Some(ManifestGateway {
                    bind: None,
                    port: Some(8000),
                    domains: None,
                }),
                artificial_delay_ms: None,
                ii: None,
                nns: None,
                subnets: None,
                bitcoind_addr: Some(vec!["127.0.0.1:18444".to_string()]),
                dogecoind_addr: None,
                version: None,
            }),
        });

        let config: Configuration = mode.into();
        match config {
            Configuration::Managed {
                managed:
                    Managed {
                        mode: ManagedMode::Launcher(launcher_config),
                    },
            } => {
                assert_eq!(
                    launcher_config.bitcoind_addr,
                    Some(vec!["127.0.0.1:18444".to_string()])
                );
                assert_eq!(launcher_config.dogecoind_addr, None);
                assert!(!launcher_config.ii);
                assert!(!launcher_config.nns);
            }
            _ => panic!("expected ManagedMode::Launcher"),
        }
    }
}
