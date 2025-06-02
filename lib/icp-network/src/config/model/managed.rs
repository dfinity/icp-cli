use serde::{Deserialize, Deserializer};

/// A "managed network" is a network that we start, configure, stop.
#[derive(Deserialize, Default)]
pub struct ManagedNetworkModel {
    pub gateway: GatewayModel,
}

#[derive(Deserialize)]
pub struct GatewayModel {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port", deserialize_with = "deserialize_port")]
    pub port: BindPort,
}

#[derive(Debug, Clone, Deserialize)]
pub enum BindPort {
    Fixed(u16),
    Dynamic,
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
        BindPort::Dynamic
    } else {
        BindPort::Fixed(raw)
    })
}
