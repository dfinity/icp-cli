//! Network *configuration* model (the manifest-derived view of a network).
//!
//! Runtime concerns — launching/stopping managed networks, network descriptors,
//! agent access — live in the host `icp` crate.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use strum::EnumString;
use url::Url;

pub use crate::manifest::network::RootKeySpec;
use crate::manifest::network::{
    Connected as ManifestConnected, Endpoints, Gateway as ManifestGateway, Mode,
};

pub const DEFAULT_LOCAL_NETWORK_BIND: &str = "127.0.0.1";
pub const DEFAULT_LOCAL_NETWORK_PORT: u16 = 8000;

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
