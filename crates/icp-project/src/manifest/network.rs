use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::network::SubnetKind;

/// A network definition for the project
#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct NetworkManifest {
    pub name: String,

    #[serde(flatten)]
    pub configuration: Mode,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", tag = "mode")]
pub enum Mode {
    Managed(Managed),
    Connected(Connected),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Managed {
    #[serde(flatten)]
    pub mode: Box<ManagedMode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(untagged, rename_all_fields = "kebab-case")]
#[allow(clippy::large_enum_variant)]
pub enum ManagedMode {
    Image {
        /// The docker image to use for the network
        image: String,
        /// Port mappings in the format "host_port:container_port"
        port_mapping: Vec<String>,
        /// Whether to delete the container when the network stops
        #[serde(skip_serializing_if = "Option::is_none")]
        rm_on_exit: Option<bool>,
        /// Command line arguments to pass to the container's entrypoint
        #[serde(
            alias = "cmd",
            alias = "command",
            skip_serializing_if = "Option::is_none"
        )]
        args: Option<Vec<String>>,
        /// Entrypoint to use for the container
        #[serde(skip_serializing_if = "Option::is_none")]
        entrypoint: Option<Vec<String>>,
        /// Environment variables to set in the container in VAR=VALUE format (or VAR to inherit from host)
        #[serde(skip_serializing_if = "Option::is_none")]
        environment: Option<Vec<String>>,
        /// Volumes to mount into the container in the format name:container_path[:options]
        #[serde(skip_serializing_if = "Option::is_none")]
        volumes: Option<Vec<String>>,
        /// The platform to use for the container (e.g. linux/amd64)
        #[serde(skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
        /// The user to run the container as in the format user[:group]
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        /// The size of /dev/shm in bytes
        #[serde(skip_serializing_if = "Option::is_none")]
        shm_size: Option<i64>,
        /// The status directory inside the container. Defaults to /app/status
        #[serde(skip_serializing_if = "Option::is_none")]
        status_dir: Option<String>,
        /// Bind mounts to add to the container in the format relative_host_path:container_path[:options]
        #[serde(skip_serializing_if = "Option::is_none")]
        mounts: Option<Vec<String>>,
        /// Extra hosts entries for Docker networking (e.g. "host.docker.internal:host-gateway")
        #[serde(skip_serializing_if = "Option::is_none")]
        extra_hosts: Option<Vec<String>>,
    },
    Launcher {
        /// HTTP gateway configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        gateway: Option<Gateway>,
        /// Artificial delay to add to every update call
        #[serde(skip_serializing_if = "Option::is_none")]
        artificial_delay_ms: Option<u64>,
        /// Set up the Internet Identity canister. Makes internet identity available at
        /// id.ai.localhost:<port>
        #[serde(skip_serializing_if = "Option::is_none")]
        ii: Option<bool>,
        /// Set up the NNS
        #[serde(skip_serializing_if = "Option::is_none")]
        nns: Option<bool>,
        /// Configure the list of subnets (one application subnet by default)
        #[serde(skip_serializing_if = "Option::is_none")]
        subnets: Option<Vec<SubnetKind>>,
        /// Bitcoin P2P node addresses to connect to (e.g. "127.0.0.1:18444")
        #[serde(skip_serializing_if = "Option::is_none")]
        bitcoind_addr: Option<Vec<String>>,
        /// Dogecoin P2P node addresses to connect to
        #[serde(skip_serializing_if = "Option::is_none")]
        dogecoind_addr: Option<Vec<String>>,
        /// The version of icp-cli-network-launcher to use. Defaults to the latest released version. Launcher versions correspond to published PocketIC or IC-OS releases.
        #[serde(skip_serializing_if = "Option::is_none")]
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
            bitcoind_addr: None,
            dogecoind_addr: None,
            version: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    #[serde(flatten)]
    pub endpoints: Endpoints,

    /// How to obtain the root key used to verify responses from this network.
    ///
    /// One of:
    /// - `mainnet`: use the canonical IC mainnet root key (e.g. to reach mainnet
    ///   through a non-default boundary node without repeating the key literal).
    /// - `fetch`: fetch the root key from the network on each use. This is
    ///   trust-on-first-use and does *not* verify the key's provenance; only use it
    ///   for testnets that you (or someone you trust) operate.
    /// - a 266-character hex-encoded root key (133 bytes).
    pub root_key: RootKeySpec,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(untagged, rename_all_fields = "kebab-case")]
pub enum Endpoints {
    Explicit {
        /// The URL of the HTTP gateway endpoint. Should support prefixing canister IDs as subdomains,
        /// otherwise icp-cli will fall back to ?canisterId= query parameters which are frequently brittle in frontend code.
        ///
        /// If no HTTP gateway endpoint is provided, canister URLs will not be printed in deploy operations.
        #[serde(skip_serializing_if = "Option::is_none")]
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

/// The expected byte length of a root key (133 bytes / 266 hex characters).
pub const ROOT_KEY_LEN: usize = 133;

/// How to obtain the root key used to verify responses from a connected network.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(try_from = "String", into = "String")]
pub enum RootKeySpec {
    /// Use the canonical IC mainnet root key.
    Mainnet,
    /// Fetch the root key from the network on each use (trust-on-first-use, unverified).
    Fetch,
    /// A specific root key (133 bytes).
    Explicit(Vec<u8>),
}

impl TryFrom<String> for RootKeySpec {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "mainnet" => Ok(RootKeySpec::Mainnet),
            "fetch" => Ok(RootKeySpec::Fetch),
            hex => {
                let bytes = hex::decode(hex)
                    .map_err(|e| format!("invalid root key: expected \"mainnet\", \"fetch\", or a hex-encoded key, but failed to decode as hex: {e}"))?;
                if bytes.len() != ROOT_KEY_LEN {
                    return Err(format!(
                        "invalid root key: expected {ROOT_KEY_LEN} bytes but got {}",
                        bytes.len()
                    ));
                }
                Ok(RootKeySpec::Explicit(bytes))
            }
        }
    }
}

impl From<RootKeySpec> for String {
    fn from(spec: RootKeySpec) -> Self {
        match spec {
            RootKeySpec::Mainnet => "mainnet".to_string(),
            RootKeySpec::Fetch => "fetch".to_string(),
            RootKeySpec::Explicit(bytes) => hex::encode(bytes),
        }
    }
}

impl JsonSchema for RootKeySpec {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RootKeySpec".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Root key: \"mainnet\", \"fetch\", or a 266-character hex-encoded key.",
            "anyOf": [
                { "enum": ["mainnet", "fetch"] },
                { "pattern": "^[0-9a-f]{266}$" }
            ]
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct Gateway {
    /// Network interface for the gateway. Defaults to 127.0.0.1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    /// Domains the gateway should respond to. Automatically includes localhost if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    /// Port for the gateway to listen on. Defaults to 8000
    #[serde(skip_serializing_if = "Option::is_none")]
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
                root-key: mainnet
            "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Connected(Connected {
                    endpoints: Endpoints::Implicit {
                        url: "https://ic0.app".parse().unwrap(),
                    },
                    root_key: RootKeySpec::Mainnet,
                }),
            },
        );
    }

    #[test]
    fn connected_network_fetch() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                name: my-network
                mode: connected
                url: https://testnet.example.com
                root-key: fetch
            "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Connected(Connected {
                    endpoints: Endpoints::Implicit {
                        url: "https://testnet.example.com".parse().unwrap(),
                    },
                    root_key: RootKeySpec::Fetch,
                }),
            },
        );
    }

    #[test]
    fn connected_network_requires_root_key() {
        match serde_yaml::from_str::<NetworkManifest>(indoc! {r#"
                name: my-network
                mode: connected
                url: https://ic0.app
            "#})
        {
            Ok(_) => panic!("a connected network without a root key should fail"),
            Err(err) => {
                if !format!("{err}").contains("root-key") {
                    panic!("unexpected error for missing root key: {err}");
                }
            }
        };
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
                    root_key: RootKeySpec::try_from(
                        "308182301d060d2b0601040182dc7c0503010201060c2b0601040182dc7c050302010\
                            361008b52b4994f94c7ce4be1c1542d7c81dc79fea17d49efe8fa42e8566373581d4b9\
                            69c4a59e96a0ef51b711fe5027ec01601182519d0a788f4bfe388e593b97cd1d7e4490\
                            4de79422430bca686ac8c21305b3397b5ba4d7037d17877312fb7ee34"
                            .to_string()
                    )
                    .unwrap(),
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
                        bitcoind_addr: None,
                        dogecoind_addr: None,
                        version: None,
                    })
                })
            },
        );
    }

    #[test]
    fn managed_network_with_bind() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                    gateway:
                      bind: 127.0.0.1
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Managed(Managed {
                    mode: Box::new(ManagedMode::Launcher {
                        gateway: Some(Gateway {
                            bind: Some("127.0.0.1".to_string()),
                            domains: None,
                            port: None,
                        }),
                        artificial_delay_ms: None,
                        ii: None,
                        nns: None,
                        subnets: None,
                        bitcoind_addr: None,
                        dogecoind_addr: None,
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
                      bind: 127.0.0.1
                      port: 8000
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Managed(Managed {
                    mode: Box::new(ManagedMode::Launcher {
                        gateway: Some(Gateway {
                            bind: Some("127.0.0.1".to_string()),
                            domains: None,
                            port: Some(8000)
                        }),
                        artificial_delay_ms: None,
                        ii: None,
                        nns: None,
                        subnets: None,
                        bitcoind_addr: None,
                        dogecoind_addr: None,
                        version: None,
                    })
                })
            },
        );
    }

    #[test]
    fn managed_network_with_dogecoind_addr() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                    dogecoind-addr:
                      - "127.0.0.1:22556"
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
                        bitcoind_addr: None,
                        dogecoind_addr: Some(vec!["127.0.0.1:22556".to_string()]),
                        version: None,
                    })
                })
            },
        );
    }

    #[test]
    fn managed_docker_network_with_extra_hosts() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                    image: ghcr.io/dfinity/icp-cli-network-launcher
                    port-mapping:
                      - "8000:4943"
                    extra-hosts:
                      - "host.docker.internal:host-gateway"
                "#}),
            NetworkManifest {
                name: "my-network".to_string(),
                configuration: Mode::Managed(Managed {
                    mode: Box::new(ManagedMode::Image {
                        image: "ghcr.io/dfinity/icp-cli-network-launcher".to_string(),
                        port_mapping: vec!["8000:4943".to_string()],
                        rm_on_exit: None,
                        args: None,
                        entrypoint: None,
                        environment: None,
                        volumes: None,
                        platform: None,
                        user: None,
                        shm_size: None,
                        status_dir: None,
                        mounts: None,
                        extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
                    })
                })
            },
        );
    }

    #[test]
    fn managed_network_with_bitcoind_addr() {
        assert_eq!(
            validate_network_yaml(indoc! {r#"
                    name: my-network
                    mode: managed
                    bitcoind-addr:
                      - "127.0.0.1:18444"
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
                        bitcoind_addr: Some(vec!["127.0.0.1:18444".to_string()]),
                        dogecoind_addr: None,
                        version: None,
                    })
                })
            },
        );
    }
}
