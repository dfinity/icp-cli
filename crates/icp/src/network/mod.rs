pub mod access;
pub mod config;
mod directory;
mod lock;
mod managed;
pub mod structure;

pub use directory::NetworkDirectory;
pub use managed::run::{RunNetworkError, run_network};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub enum Port {
    Fixed(u16),
    Random,
}

impl Default for Port {
    fn default() -> Self {
        Port::Fixed(8080)
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

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub struct Managed {
    pub gateway: Gateway,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    /// The URL this network can be reached at.
    pub url: String,

    /// The root key of this network
    pub root_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Configuration {
    /// A managed network is one which can be controlled and manipulated.
    Managed(Managed),

    /// A connected network is one which can be interacted with
    /// but cannot be controlled or manipulated.
    Connected(Connected),
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration::Managed(Managed::default())
    }
}
