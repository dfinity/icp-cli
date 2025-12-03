use schemars::JsonSchema;
use serde::Deserialize;

/// A network definition for the project
#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize)]
pub struct NetworkManifest {
    pub name: String,

    #[serde(flatten)]
    pub configuration: Mode,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize)]
#[serde(rename_all = "lowercase", tag = "mode")]
pub enum Mode {
    Managed(Managed),
    Connected(Connected),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub struct Managed {
    pub gateway: Option<Gateway>,
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
pub struct Gateway {
    pub host: Option<String>,
    pub port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    /// Validates network yaml against the schema and deserializes it to a manifest
    fn validate_network_yaml(s: &str) -> NetworkManifest {
        let schema = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../docs/schemas/network-yaml-schema.json"
        ))
        .expect("failed to deserialize network.yaml schema");
        let network_yaml = serde_yaml::from_str::<serde_json::Value>(s)
            .expect("failed to deserialize network.yaml");

        // Build & reuse
        let validator = jsonschema::options()
            .build(&schema)
            .expect("failed to build jsonschema validator");

        // Iterate over errors
        for error in validator.iter_errors(&network_yaml) {
            eprintln!("Error: {error:#?}");
        }

        assert!(validator.is_valid(&network_yaml));

        serde_yaml::from_str::<NetworkManifest>(s)
            .expect("failed to deserialize NetworkManifest from yaml")
    }

    #[test]
    fn connected_network() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                name: my-network
                mode: connected
                url: https://ic0.app
            "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Connected(Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: None
                }),
            },
        );
    }

    #[test]
    fn just_a_name_fails() {
        match serde_yaml::from_str::<NetworkManifest>(r#"name: my-network"#) {
            // No Error
            Ok(_) => {
                panic!("an incomplete network definition should result in an error");
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}").starts_with("missing field `mode`") {
                    panic!("an incomplete network definition resulted in the wrong error: {err}");
                };
            }
        };
    }

    #[test]
    fn connected_network_with_key() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: connected
                    url: https://ic0.app
                    root-key: the-key
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Connected(Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: Some("the-key".to_string())
                }),
            },
        );
    }

    #[test]
    fn managed_network() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Managed(Managed { gateway: None })
            },
        );
    }

    #[test]
    fn managed_network_with_host() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                    gateway:
                      host: localhost
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Managed(Managed {
                    gateway: Some(Gateway {
                        host: Some("localhost".to_string()),
                        port: None,
                    })
                })
            },
        );
    }

    #[test]
    fn managed_network_with_port() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                    gateway:
                      host: localhost
                      port: 8000
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Managed(Managed {
                    gateway: Some(Gateway {
                        host: Some("localhost".to_string()),
                        port: Some(8000)
                    })
                })
            },
        );
    }
}
