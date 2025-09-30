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

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct NetworkInner {
    pub name: String,

    #[serde(flatten)]
    pub configuration: Option<Configuration>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct Network {
    pub name: String,
    pub configuration: Configuration,
}

impl From<NetworkInner> for Network {
    fn from(v: NetworkInner) -> Self {
        let NetworkInner {
            name,
            configuration,
        } = v;

        // Configuration
        let configuration = configuration.unwrap_or_default();

        Network {
            name,
            configuration,
        }
    }
}

impl<'de> Deserialize<'de> for Network {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: NetworkInner = Deserialize::deserialize(d)?;
        Ok(inner.into())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Error;

    use super::*;

    #[test]
    fn default_configuration() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Network>(
                r#"
                name: my-network
                "#
            )?,
            Network {
                name: "my-network".to_string(),
                configuration: Configuration::Managed(Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8080),
                    }
                })
            },
        );

        Ok(())
    }

    #[test]
    fn connected_network() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Network>(
                r#"
                name: my-network
                mode: connected
                url: https://ic0.app
                "#
            )?,
            Network {
                name: "my-network".to_string(),
                configuration: Configuration::Connected(Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: None,
                })
            },
        );

        Ok(())
    }

    #[test]
    fn connected_network_with_key() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Network>(
                r#"
                name: my-network
                mode: connected
                url: https://ic0.app
                root-key: root-key
                "#
            )?,
            Network {
                name: "my-network".to_string(),
                configuration: Configuration::Connected(Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: Some("root-key".to_string()),
                })
            },
        );

        Ok(())
    }

    #[test]
    fn managed_network() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Network>(
                r#"
                name: my-network
                mode: managed
                "#
            )?,
            Network {
                name: "my-network".to_string(),
                configuration: Configuration::Managed(Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8080),
                    }
                })
            },
        );

        Ok(())
    }

    #[test]
    fn managed_network_with_host_port() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Network>(
                r#"
                name: my-network
                mode: managed
                gateway:
                  host: my-host
                  port: 1234
                "#
            )?,
            Network {
                name: "my-network".to_string(),
                configuration: Configuration::Managed(Managed {
                    gateway: Gateway {
                        host: "my-host".to_string(),
                        port: Port::Fixed(1234),
                    }
                })
            },
        );

        Ok(())
    }

    #[test]
    fn managed_network_with_random_port() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Network>(
                r#"
                name: my-network
                mode: managed
                gateway:
                  port: 0
                "#
            )?,
            Network {
                name: "my-network".to_string(),
                configuration: Configuration::Managed(Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Random,
                    }
                })
            },
        );

        Ok(())
    }
}
