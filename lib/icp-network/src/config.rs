use camino::Utf8PathBuf;
use candid::Principal;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteField {
    /// Single url
    Url(String),

    /// More than one url (route round-robin)
    Urls(Vec<String>),
}

/// A "connected network" is a network that we connect to but don't manage.
/// Typical examples are mainnet or testnets.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectedNetworkModel {
    /// The URL(s) this network can be reached at.
    #[serde(flatten)]
    pub route: RouteField,

    /// The root key of this network
    pub root_key: Option<String>,
}

/// A "managed network" is a network that we start, configure, stop.
#[derive(Clone, Debug, Deserialize, Default)]
pub struct ManagedNetworkModel {
    pub gateway: GatewayModel,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GatewayModel {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port", deserialize_with = "deserialize_port")]
    pub port: BindPort,
}

#[derive(Debug, Clone, Deserialize)]
pub enum BindPort {
    Fixed(u16),
    Random,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> BindPort {
    BindPort::Fixed(8000)
}

impl Default for GatewayModel {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn deserialize_port<'de, D>(deserializer: D) -> Result<BindPort, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = u16::deserialize(deserializer)?;
    Ok(if raw == 0 {
        BindPort::Random
    } else {
        BindPort::Fixed(raw)
    })
}

pub type NetworkName = String;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum NetworkConfig {
    Managed(ManagedNetworkModel),
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
            Some(self.gateway.port)
        } else {
            None
        }
    }
}
