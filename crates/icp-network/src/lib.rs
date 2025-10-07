use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

pub mod access;
pub mod config;
mod directory;
mod lock;
pub mod managed;
pub mod status;
pub mod structure;

pub use directory::NetworkDirectory;
pub use managed::run::{RunNetworkError, run_network};

pub const NETWORK_LOCAL: &str = "local";
pub const NETWORK_IC: &str = "ic";

/// A "connected network" is a network that we connect to but don't manage.
/// Typical examples are mainnet or testnets.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectedNetworkModel {
    /// The URL this network can be reached at.
    pub url: String,

    /// The root key of this network
    pub root_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub enum BindPort {
    Fixed(u16),
    Random,
}

fn deserialize_port<'de, D>(deserializer: D) -> Result<BindPort, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match u16::deserialize(deserializer)? {
        0 => BindPort::Random,
        p => BindPort::Fixed(p),
    })
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> BindPort {
    BindPort::Fixed(8000)
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct GatewayModel {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port", deserialize_with = "deserialize_port")]
    pub port: BindPort,
}

/// A "managed network" is a network that we start, configure, stop.
#[derive(Clone, Debug, Deserialize, Default, JsonSchema)]
pub struct ManagedNetworkModel {
    pub gateway: GatewayModel,
}

impl Default for GatewayModel {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum NetworkConfig {
    /// A managed network is one which can be controlled and manipulated.
    Managed(ManagedNetworkModel),

    /// A connected network is one which can be interacted with
    /// but cannot be controlled or manipulated.
    Connected(ConnectedNetworkModel),
}

impl NetworkConfig {
    pub fn local_default() -> Self {
        NetworkConfig::Managed(ManagedNetworkModel::default())
    }
}
