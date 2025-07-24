use camino::Utf8PathBuf;
use candid::Principal;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// A "connected network" is a network that we connect to but don't manage.
/// Typical examples are mainnet or testnets.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectedNetworkModel {
    /// The URL this network can be reached at.
    pub url: String,

    /// The root key of this network
    pub root_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct GatewayModel {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port", deserialize_with = "deserialize_port")]
    pub port: BindPort,
}

/// A "managed network" is a network that we start, configure, stop.
#[derive(Clone, Debug, Deserialize, Default)]
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

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorGatewayPort {
    pub fixed: bool,
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorModel {
    pub id: Uuid,
    pub project_dir: Utf8PathBuf,
    pub network: String,
    pub network_dir: Utf8PathBuf,
    pub gateway: NetworkDescriptorGatewayPort,
    pub default_effective_canister_id: Principal,
    pub pid: Option<u32>,
    pub root_key: String,
}

impl NetworkDescriptorModel {
    pub fn gateway_port(&self) -> Option<u16> {
        if self.gateway.fixed {
            return Some(self.gateway.port);
        }

        None
    }
}
