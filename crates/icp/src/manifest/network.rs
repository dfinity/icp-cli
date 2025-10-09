use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::network::Configuration;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct NetworkInner {
    pub name: String,

    #[serde(flatten)]
    pub configuration: Option<Configuration>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct NetworkManifest {
    pub name: String,
    pub configuration: Configuration,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Overriding the local network is not supported.")]
    OverrideLocal,

    #[error("Overriding the mainnet network is not supported.")]
    OverrideMainnet,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

impl TryFrom<NetworkInner> for NetworkManifest {
    type Error = ParseError;

    fn try_from(v: NetworkInner) -> Result<Self, Self::Error> {
        let NetworkInner {
            name,
            configuration,
        } = v;

        // Name
        if name == "local" {
            return Err(ParseError::OverrideLocal);
        }

        if name == "mainnet" {
            return Err(ParseError::OverrideMainnet);
        }

        // Configuration
        let configuration = configuration.unwrap_or_default();

        Ok(NetworkManifest {
            name,
            configuration,
        })
    }
}

impl<'de> Deserialize<'de> for NetworkManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: NetworkInner = Deserialize::deserialize(d)?;
        inner.try_into().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Error, anyhow};

    use crate::network::{Connected, Gateway, Managed, Port};

    use super::*;

    #[test]
    fn default_configuration() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<NetworkManifest>(
                r#"
                name: my-network
                "#
            )?,
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Configuration::Managed(Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8000),
                    }
                })
            },
        );

        Ok(())
    }

    #[test]
    fn override_local() -> Result<(), Error> {
        match serde_yaml::from_str::<NetworkManifest>(r#"name: local"#) {
            // No Error
            Ok(_) => {
                return Err(anyhow!("a network named local should result in an error"));
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}").starts_with("Overriding the local network") {
                    return Err(anyhow!(
                        "a network named local resulted in the wrong error: {err}"
                    ));
                };
            }
        };

        Ok(())
    }

    #[test]
    fn override_mainnet() -> Result<(), Error> {
        match serde_yaml::from_str::<NetworkManifest>(r#"name: mainnet"#) {
            // No Error
            Ok(_) => {
                return Err(anyhow!("a network named mainnet should result in an error"));
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}").starts_with("Overriding the mainnet network") {
                    return Err(anyhow!(
                        "a network named mainnet resulted in the wrong error: {err}"
                    ));
                };
            }
        };

        Ok(())
    }

    #[test]
    fn connected_network() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<NetworkManifest>(
                r#"
                name: my-network
                mode: connected
                url: https://ic0.app
                "#
            )?,
            NetworkManifest {
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
            serde_yaml::from_str::<NetworkManifest>(
                r#"
                name: my-network
                mode: connected
                url: https://ic0.app
                root-key: root-key
                "#
            )?,
            NetworkManifest {
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
            serde_yaml::from_str::<NetworkManifest>(
                r#"
                name: my-network
                mode: managed
                "#
            )?,
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Configuration::Managed(Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8000),
                    }
                })
            },
        );

        Ok(())
    }

    #[test]
    fn managed_network_with_host_port() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<NetworkManifest>(
                r#"
                name: my-network
                mode: managed
                gateway:
                  host: my-host
                  port: 1234
                "#
            )?,
            NetworkManifest {
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
            serde_yaml::from_str::<NetworkManifest>(
                r#"
                name: my-network
                mode: managed
                gateway:
                  port: 0
                "#
            )?,
            NetworkManifest {
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
