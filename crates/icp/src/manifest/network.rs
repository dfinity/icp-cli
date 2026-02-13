use schemars::JsonSchema;
use serde::Deserialize;
use url::Url;

use crate::network::SubnetKind;

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
        /// The docker image to use for the network
        image: String,
        /// Port mappings in the format "host_port:container_port"
        port_mapping: Vec<String>,
        /// Whether to delete the container when the network stops
        rm_on_exit: Option<bool>,
        /// Command line arguments to pass to the container's entrypoint
        #[serde(alias = "cmd", alias = "command")]
        args: Option<Vec<String>>,
        /// Entrypoint to use for the container
        entrypoint: Option<Vec<String>>,
        /// Environment variables to set in the container in VAR=VALUE format (or VAR to inherit from host)
        environment: Option<Vec<String>>,
        /// Volumes to mount into the container in the format name:container_path[:options]
        volumes: Option<Vec<String>>,
        /// The platform to use for the container (e.g. linux/amd64)
        platform: Option<String>,
        /// The user to run the container as in the format user[:group]
        user: Option<String>,
        /// The size of /dev/shm in bytes
        shm_size: Option<i64>,
        /// The status directory inside the container. Defaults to /app/status
        status_dir: Option<String>,
        /// Bind mounts to add to the container in the format relative_host_path:container_path[:options]
        mounts: Option<Vec<String>>,
    },
    Launcher {
        /// HTTP gateway configuration
        gateway: Option<Gateway>,
        /// Artificial delay to add to every update call
        artificial_delay_ms: Option<u64>,
        /// Set up the Internet Identity canister
        ii: Option<bool>,
        /// Set up the NNS
        nns: Option<bool>,
        /// Configure the list of subnets (one application subnet by default)
        subnets: Option<Vec<SubnetKind>>,
        /// The version of icp-cli-network-launcher to use. Defaults to the latest released version. Launcher versions correspond to published PocketIC or IC-OS releases.
        version: Option<String>,
    },
}

impl Default for ManagedMode {
    fn default() -> Self {
        ManagedMode::Launcher {
            gateway: None,
            artificial_delay_ms: None,
            ii: None,
            nns: None,
            subnets: None,
            version: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    #[serde(flatten)]
    pub endpoints: Endpoints,

    /// The root key of this network
    #[schemars(with = "Option<String>", regex(pattern = "^[0-9a-f]{266}$"))]
    pub root_key: Option<RootKey>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged, rename_all_fields = "kebab-case")]
pub enum Endpoints {
    Explicit {
        /// The URL of the HTTP gateway endpoint. Should support prefixing canister IDs as subdomains,
        /// otherwise icp-cli will fall back to ?canisterId= query parameters which are frequently brittle in frontend code.
        ///
        /// If no HTTP gateway endpoint is provided, canister URLs will not be printed in deploy operations.
        http_gateway_url: Option<Url>,
        /// The URL of the API endpoint. Should support the standard API routes (e.g. /api/v3).
        api_url: Url,
    },
    Implicit {
        /// The URL this network can be reached at.
        ///
        /// Assumed to be the URL of both the HTTP gateway (canister-id.domain.com) and API (domain.com/api/v3).
        url: Url,
    },
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
                    endpoints: Endpoints::Implicit {
                        url: "https://ic0.app".parse().unwrap(),
                    },
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
                    endpoints: Endpoints::Implicit {
                        url: "https://ic0.app".parse().unwrap(),
                    },
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
                    mode: Box::new(ManagedMode::Launcher {
                        gateway: None,
                        artificial_delay_ms: None,
                        ii: None,
                        nns: None,
                        subnets: None,
                        version: None,
                    })
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
                        }),
                        artificial_delay_ms: None,
                        ii: None,
                        nns: None,
                        subnets: None,
                        version: None,
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
                        }),
                        artificial_delay_ms: None,
                        ii: None,
                        nns: None,
                        subnets: None,
                        version: None,
                    })
                })
            },
        );
    }
}
