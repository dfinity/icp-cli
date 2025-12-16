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
    #[serde(flatten)]
    pub mode: Box<ManagedMode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged, rename_all_fields = "kebab-case")]
#[allow(clippy::large_enum_variant)]
pub enum ManagedMode {
    Image {
        image: String,
        port_mapping: Vec<String>,
        rm_on_exit: Option<bool>,
        #[serde(alias = "cmd", alias = "command")]
        args: Option<Vec<String>>,
        entrypoint: Option<Vec<String>>,
        environment: Option<Vec<String>>,
        volumes: Option<Vec<String>>,
        platform: Option<String>,
        user: Option<String>,
        shm_size: Option<i64>,
        status_dir: Option<String>,
        mounts: Option<Vec<String>>,
    },
    Launcher {
        gateway: Option<Gateway>,
    },
}

impl Default for ManagedMode {
    fn default() -> Self {
        ManagedMode::Launcher { gateway: None }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    /// The URL this network can be reached at.
    pub url: String,

    /// The root key of this network
    #[schemars(with = "Option<String>", regex(pattern = "^[0-9a-f]{266}$"))]
    pub root_key: Option<RootKey>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(try_from = "String", into = "String")]
pub struct RootKey(pub Vec<u8>);

impl TryFrom<String> for RootKey {
    type Error = hex::FromHexError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = hex::decode(value)?;
        Ok(RootKey(bytes))
    }
}

impl From<RootKey> for String {
    fn from(root_key: RootKey) -> Self {
        hex::encode(root_key.0)
    }
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
        #[rustfmt::skip] // https://github.com/rust-lang/rustfmt/issues/6747
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: connected
                    url: https://ic0.app
                    root-key: "308182301d060d2b0601040182dc7c0503010201060c2b0601040182dc7c0503020\
                      10361008b52b4994f94c7ce4be1c1542d7c81dc79fea17d49efe8fa42e8566373581d4b969c4\
                      a59e96a0ef51b711fe5027ec01601182519d0a788f4bfe388e593b97cd1d7e44904de7942243\
                      0bca686ac8c21305b3397b5ba4d7037d17877312fb7ee34"
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Connected(Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: Some(
                        RootKey::try_from(
                            "308182301d060d2b0601040182dc7c0503010201060c2b0601040182dc7c050302010\
                            361008b52b4994f94c7ce4be1c1542d7c81dc79fea17d49efe8fa42e8566373581d4b9\
                            69c4a59e96a0ef51b711fe5027ec01601182519d0a788f4bfe388e593b97cd1d7e4490\
                            4de79422430bca686ac8c21305b3397b5ba4d7037d17877312fb7ee34"
                                .to_string()
                        )
                        .unwrap()
                    )
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
                configuration: Mode::Managed(Managed {
                    mode: Box::new(ManagedMode::Launcher { gateway: None })
                })
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
                    mode: Box::new(ManagedMode::Launcher {
                        gateway: Some(Gateway {
                            host: Some("localhost".to_string()),
                            port: None,
                        })
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
                    mode: Box::new(ManagedMode::Launcher {
                        gateway: Some(Gateway {
                            host: Some("localhost".to_string()),
                            port: Some(8000)
                        })
                    })
                })
            },
        );
    }
}
