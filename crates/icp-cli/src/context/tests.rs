//! Tests for the identity- and agent-resolution the CLI layers on top of the
//! library context.

use std::{collections::HashMap, sync::Arc};

use ic_agent::Identity;
use icp::{
    Environment, MockProjectLoader, Network, NoProjectLoader, Project,
    context::{EnvironmentSelection, NetworkSelection},
    network::{
        Configuration, Gateway, Managed, ManagedLauncherConfig, ManagedMode, MockNetworkAccessor,
        Port, access::NetworkAccess,
    },
    prelude::*,
};
use indexmap::IndexMap;
use url::Url;

use super::*;
use crate::identity::MockIdentityLoader;

const DEFAULT_LOCAL_NETWORK_URL: &str = "http://localhost:8000";

#[tokio::test]
async fn test_get_identity_default() {
    let ctx = Context::mocked();

    let result = ctx.get_identity(&IdentitySelection::Default, None).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_identity_anonymous() {
    let ctx = Context::mocked();

    let result = ctx.get_identity(&IdentitySelection::Anonymous, None).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_identity_named() {
    let alice_identity: Arc<dyn Identity> = Arc::new(ic_agent::identity::AnonymousIdentity);

    let ctx = Context {
        identity: Arc::new(
            MockIdentityLoader::anonymous().with_identity("alice", Arc::clone(&alice_identity)),
        ),
        ..Context::mocked()
    };

    let result = ctx
        .get_identity(&IdentitySelection::Named("alice".to_string()), None)
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_identity_named_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_identity(&IdentitySelection::Named("nonexistent".to_string()), None)
        .await;

    assert!(matches!(
        result,
        Err(GetIdentityError::IdentityLoad {
            identity: IdentitySelection::Named(_),
            source: crate::identity::LoadError::LoadIdentity { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_env_uses_environment_network() {
    let local_root_key = vec![1, 2, 3];
    let staging_root_key = vec![4, 5, 6];

    // Complex project has "test" environment which uses "staging" network
    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::complex()),
            network: Arc::new(
                MockNetworkAccessor::new()
                    .with_network(
                        "local",
                        NetworkAccess {
                            root_key: local_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse("http://localhost:8000").unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    )
                    .with_network(
                        "staging",
                        NetworkAccess {
                            root_key: staging_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse("http://staging:9000").unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    ),
            ),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent_for_env(
            &IdentitySelection::Anonymous,
            &EnvironmentSelection::Named("test".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(agent.read_root_key(), staging_root_key);
}

#[tokio::test]
async fn test_get_agent_for_env_environment_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_agent_for_env(
            &IdentitySelection::Anonymous,
            &EnvironmentSelection::Named("nonexistent".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(GetAgentForEnvError::GetEnvironment {
            source: icp::context::GetEnvironmentError::EnvironmentNotFound { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_env_network_not_configured() {
    // Environment "dev" exists in project and uses "local" network,
    // but "local" network is not configured in MockNetworkAccessor
    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::complex()),
            // MockNetworkAccessor has no networks configured
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let result = ctx
        .get_agent_for_env(
            &IdentitySelection::Anonymous,
            &EnvironmentSelection::Named("dev".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(GetAgentForEnvError::NetworkAccess {
            source: icp::network::AccessError::GetNetworkAccess { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_network_success() {
    let root_key = vec![1, 2, 3];

    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::complex()),
            network: Arc::new(MockNetworkAccessor::new().with_network(
                "local",
                NetworkAccess {
                    root_key: root_key.clone(),
                    root_key_source: icp::network::RootKeySource::Configured,
                    api_url: Url::parse("http://localhost:8000").unwrap(),
                    http_gateway_url: None,
                    use_friendly_domains: false,
                },
            )),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent_for_network(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Named("local".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(agent.read_root_key(), root_key);
}

#[tokio::test]
async fn test_get_agent_for_network_network_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_agent_for_network(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Named("nonexistent".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(GetAgentForNetworkError::GetNetwork {
            source: icp::context::GetNetworkError::NetworkNotFound { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_network_not_configured() {
    // Network "local" exists in project but is not configured in MockNetworkAccessor
    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::complex()),
            // MockNetworkAccessor has no networks configured
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let result = ctx
        .get_agent_for_network(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Named("local".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(GetAgentForNetworkError::NetworkAccess {
            source: icp::network::AccessError::GetNetworkAccess { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_url_success() {
    let ctx = Context::mocked();

    let result = ctx
        .get_agent_for_url(
            &IdentitySelection::Anonymous,
            &Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_agent_defaults_outside_project() {
    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(NoProjectLoader),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    // Default environment + default network outside project should error
    let error = ctx
        .get_agent(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Default,
            &EnvironmentSelection::Default,
        )
        .await
        .unwrap_err();

    // Should fail with NoProjectOrNetwork error
    assert!(matches!(error, GetAgentError::NoProjectOrNetwork));
}

#[tokio::test]
async fn test_get_agent_defaults_inside_project_with_default_local() {
    let local_root_key = vec![1, 1, 1];

    // Create a project with a "local" environment (the default environment name)
    let local_network = Network {
        name: LOCAL.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                    gateway: Gateway {
                        bind: "127.0.0.1".to_string(),
                        port: Port::Fixed(8000),
                        domains: vec![],
                    },
                    artificial_delay_ms: None,
                    ii: false,
                    nns: false,
                    subnets: None,
                    bitcoind_addr: None,
                    dogecoind_addr: None,
                    version: None,
                })),
            },
        },
    };

    let mut networks = HashMap::new();
    networks.insert(LOCAL.to_string(), local_network.clone());

    let local_env = Environment {
        name: LOCAL.to_string(),
        network: local_network,
        canisters: IndexMap::new(), // No canisters needed for get_agent test
    };

    let mut environments = HashMap::new();
    environments.insert(LOCAL.to_string(), local_env);

    let project = Project {
        dir: "/project".into(),
        canisters: IndexMap::new(), // No canisters needed for get_agent test
        networks,
        environments,
        member_missing_envs: std::collections::HashMap::new(),
    };

    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::new(project)),
            network: Arc::new(MockNetworkAccessor::new().with_network(
                LOCAL,
                NetworkAccess {
                    root_key: local_root_key.clone(),
                    root_key_source: icp::network::RootKeySource::Configured,
                    api_url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                    http_gateway_url: None,
                    use_friendly_domains: false,
                },
            )),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Default,
            &EnvironmentSelection::Default,
        )
        .await
        .unwrap();

    // Should successfully create agent using project's default environment
    assert_eq!(agent.read_root_key(), local_root_key);
}

#[tokio::test]
async fn test_get_agent_defaults_with_overridden_local_network() {
    // Create a project where "local" network is overridden to use port 9000
    let custom_local_network = Network {
        name: LOCAL.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                    gateway: Gateway {
                        bind: "127.0.0.1".to_string(),
                        port: Port::Fixed(9000),
                        domains: vec![],
                    },
                    artificial_delay_ms: None,
                    ii: false,
                    nns: false,
                    subnets: None,
                    bitcoind_addr: None,
                    dogecoind_addr: None,
                    version: None,
                })),
            },
        },
    };

    let mut networks = HashMap::new();
    networks.insert(LOCAL.to_string(), custom_local_network.clone());

    let local_env = Environment {
        name: LOCAL.to_string(),
        network: custom_local_network,
        canisters: IndexMap::new(), // No canisters needed for get_agent test
    };

    let mut environments = HashMap::new();
    environments.insert(LOCAL.to_string(), local_env);

    let project = Project {
        dir: "/project".into(),
        canisters: IndexMap::new(), // No canisters needed for get_agent test
        networks,
        environments,
        member_missing_envs: std::collections::HashMap::new(),
    };

    let custom_root_key = vec![1, 2, 3, 4];

    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::new(project)),
            network: Arc::new(MockNetworkAccessor::new().with_network(
                LOCAL,
                NetworkAccess {
                    root_key: custom_root_key.clone(),
                    root_key_source: icp::network::RootKeySource::Configured,
                    api_url: Url::parse("http://localhost:9000").unwrap(), // Custom port
                    http_gateway_url: None,
                    use_friendly_domains: false,
                },
            )),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Default,
            &EnvironmentSelection::Default,
        )
        .await
        .unwrap();

    // Should use the custom network configuration
    assert_eq!(agent.read_root_key(), custom_root_key);
}

#[tokio::test]
async fn test_get_agent_defaults_with_overridden_local_environment() {
    // Create project where "local" environment uses a custom network
    let default_local_network = Network {
        name: LOCAL.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                    gateway: Gateway {
                        bind: "127.0.0.1".to_string(),
                        port: Port::Fixed(8000),
                        domains: vec![],
                    },
                    artificial_delay_ms: None,
                    ii: false,
                    nns: false,
                    subnets: None,
                    bitcoind_addr: None,
                    dogecoind_addr: None,
                    version: None,
                })),
            },
        },
    };

    let custom_network = Network {
        name: "custom".to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                    gateway: Gateway {
                        bind: "127.0.0.1".to_string(),
                        port: Port::Fixed(7000),
                        domains: vec![],
                    },
                    artificial_delay_ms: None,
                    ii: false,
                    nns: false,
                    subnets: None,
                    bitcoind_addr: None,
                    dogecoind_addr: None,
                    version: None,
                })),
            },
        },
    };

    let mut networks = HashMap::new();
    networks.insert(LOCAL.to_string(), default_local_network);
    networks.insert("custom".to_string(), custom_network.clone());

    // "local" environment uses "custom" network
    let local_env = Environment {
        name: LOCAL.to_string(),
        network: custom_network,
        canisters: IndexMap::new(), // No canisters needed for get_agent test
    };

    let mut environments = HashMap::new();
    environments.insert(LOCAL.to_string(), local_env);

    let project = Project {
        dir: "/project".into(),
        canisters: IndexMap::new(), // No canisters needed for get_agent test
        networks,
        environments,
        member_missing_envs: std::collections::HashMap::new(),
    };

    let local_root_key = vec![1, 2, 3, 4];
    let custom_root_key = vec![5, 6, 7, 8];

    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::new(project)),
            network: Arc::new(
                MockNetworkAccessor::new()
                    .with_network(
                        LOCAL,
                        NetworkAccess {
                            root_key: local_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    )
                    .with_network(
                        "custom",
                        NetworkAccess {
                            root_key: custom_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse("http://localhost:7000").unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    ),
            ),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Default,
            &EnvironmentSelection::Default,
        )
        .await
        .unwrap();

    // Should use the custom network from the overridden environment
    assert_eq!(agent.read_root_key(), custom_root_key);
}

#[tokio::test]
async fn test_get_agent_explicit_network_inside_project() {
    let local_root_key = vec![2, 3, 4];
    let staging_root_key = vec![12, 13, 14];

    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::complex()),
            network: Arc::new(
                MockNetworkAccessor::new()
                    .with_network(
                        LOCAL,
                        NetworkAccess {
                            root_key: local_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    )
                    .with_network(
                        "staging",
                        NetworkAccess {
                            root_key: staging_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse("http://localhost:8001").unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    ),
            ),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Named("staging".to_string()),
            &EnvironmentSelection::Default,
        )
        .await
        .unwrap();

    // Should use the explicitly specified network, regardless of project
    assert_eq!(agent.read_root_key(), staging_root_key);
}

#[tokio::test]
async fn test_get_agent_explicit_environment_inside_project() {
    let local_root_key = vec![5, 6, 7];
    let staging_root_key = vec![15, 16, 17];

    // complex() has "test" environment using "staging" network
    let ctx = Context {
        inner: icp::context::Context {
            project: Arc::new(MockProjectLoader::complex()),
            network: Arc::new(
                MockNetworkAccessor::new()
                    .with_network(
                        LOCAL,
                        NetworkAccess {
                            root_key: local_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    )
                    .with_network(
                        "staging",
                        NetworkAccess {
                            root_key: staging_root_key.clone(),
                            root_key_source: icp::network::RootKeySource::Configured,
                            api_url: Url::parse("http://localhost:8001").unwrap(),
                            http_gateway_url: None,
                            use_friendly_domains: false,
                        },
                    ),
            ),
            ..icp::context::Context::mocked()
        },
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent(
            &IdentitySelection::Anonymous,
            &NetworkSelection::Default,
            &EnvironmentSelection::Named("test".to_string()),
        )
        .await
        .unwrap();

    // Should use the network from the "test" environment (which is "staging")
    assert_eq!(agent.read_root_key(), staging_root_key);
}

#[cfg(unix)]
mod cwd {
    use std::sync::Mutex;

    use camino_tempfile::Utf8TempDir;

    use super::*;

    // Serializes tests that mutate $PWD, since cargo test runs tests in parallel.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn stale_pwd_is_ignored() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let stale = Utf8TempDir::new().unwrap();
        let real = PathBuf::try_from(std::env::current_dir().unwrap()).unwrap();

        let old_pwd = std::env::var("PWD").ok();
        // SAFETY: ENV_MUTEX serializes all tests that mutate $PWD.
        unsafe { std::env::set_var("PWD", stale.path()) };

        let resolved = resolve_cwd().unwrap();

        match old_pwd {
            Some(v) => unsafe { std::env::set_var("PWD", v) },
            None => unsafe { std::env::remove_var("PWD") },
        }

        assert_eq!(
            resolved, real,
            "stale $PWD should be ignored in favour of getcwd()"
        );
    }
}
