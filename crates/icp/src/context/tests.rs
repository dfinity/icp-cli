use super::*;
use crate::{
    Environment, MockProjectLoader, Network, Project,
    identity::MockIdentityLoader,
    network::{
        Configuration, Gateway, Managed, ManagedMode, MockNetworkAccessor, Port,
        access::NetworkAccess,
    },
    project::{DEFAULT_LOCAL_NETWORK_NAME, DEFAULT_LOCAL_NETWORK_URL},
    store_id::{Access as IdAccess, mock::MockInMemoryIdStore},
};
use candid::Principal;
use std::collections::HashMap;

#[tokio::test]
async fn test_get_identity_default() {
    let ctx = Context::mocked();

    let result = ctx.get_identity(&IdentitySelection::Default).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_identity_anonymous() {
    let ctx = Context::mocked();

    let result = ctx.get_identity(&IdentitySelection::Anonymous).await;

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
        .get_identity(&IdentitySelection::Named("alice".to_string()))
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_identity_named_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_identity(&IdentitySelection::Named("nonexistent".to_string()))
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
async fn test_get_environment_success() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    let env = ctx
        .get_environment(&EnvironmentSelection::Named("dev".to_string()))
        .await
        .unwrap();

    assert_eq!(env.name, "dev");
}

#[tokio::test]
async fn test_get_environment_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_environment(&EnvironmentSelection::Named("nonexistent".to_string()))
        .await;

    assert!(matches!(
        result,
        Err(GetEnvironmentError::EnvironmentNotFound { ref name }) if name == "nonexistent"
    ));
}

#[tokio::test]
async fn test_get_network_success() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    let network = ctx
        .get_network(&NetworkSelection::Named(
            DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(network.name, DEFAULT_LOCAL_NETWORK_NAME);
}

#[tokio::test]
async fn test_get_network_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_network(&NetworkSelection::Named("nonexistent".to_string()))
        .await;

    assert!(matches!(
        result,
        Err(GetNetworkError::NetworkNotFound { ref name }) if name == "nonexistent"
    ));
}

#[tokio::test]
async fn test_get_canister_id_for_env_success() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());

    // Register a canister ID for the dev environment
    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
    ids_store
        .register(true, "dev", "backend", canister_id)
        .unwrap();

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store,
        ..Context::mocked()
    };

    let cid = ctx
        .get_canister_id_for_env(
            &CanisterSelection::Named("backend".to_string()),
            &EnvironmentSelection::Named("dev".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(cid, canister_id);
}

#[tokio::test]
async fn test_get_canister_id_for_env_canister_not_in_env() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    // "database" is only in "dev" environment, not in "test"
    let result = ctx
        .get_canister_id_for_env(
            &CanisterSelection::Named("database".to_string()),
            &EnvironmentSelection::Named("test".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(GetCanisterIdForEnvError::CanisterNotFoundInEnv {
            ref canister_name,
            ref environment_name,
        }) if canister_name == "database" && environment_name == "test"
    ));
}

#[tokio::test]
async fn test_get_canister_id_for_env_id_not_registered() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    // Environment exists and canister is in it, but ID not registered
    let result = ctx
        .get_canister_id_for_env(
            &CanisterSelection::Named("backend".to_string()),
            &EnvironmentSelection::Named("dev".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(GetCanisterIdForEnvError::CanisterIdLookup {
            ref canister_name,
            ref environment_name,
            ..
        }) if canister_name == "backend" && environment_name == "dev"
    ));
}

#[tokio::test]
async fn test_set_canister_id_for_env_success() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store.clone() as Arc<dyn IdAccess>,
        ..Context::mocked()
    };

    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();

    // Set the canister ID
    ctx.set_canister_id_for_env(
        "backend",
        canister_id,
        &EnvironmentSelection::Named("dev".to_string()),
    )
    .await
    .unwrap();

    // Verify it was registered by reading it back
    let registered_id = ids_store.lookup(true, "dev", "backend").unwrap();

    assert_eq!(registered_id, canister_id);
}

#[tokio::test]
async fn test_set_canister_id_for_env_canister_not_in_env() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();

    // "database" is only in "dev" environment, not in "test"
    let result = ctx
        .set_canister_id_for_env(
            "database",
            canister_id,
            &EnvironmentSelection::Named("test".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(SetCanisterIdForEnvError::SetCanisterNotFoundInEnv {
            ref canister_name,
            ref environment_name,
        }) if canister_name == "database" && environment_name == "test"
    ));
}

#[tokio::test]
async fn test_set_canister_id_for_env_already_registered() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());

    // Pre-register a canister ID
    let first_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
    ids_store
        .register(true, "dev", "backend", first_id)
        .unwrap();

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store,
        ..Context::mocked()
    };

    // Try to register a different ID for the same canister
    let second_id = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    let result = ctx
        .set_canister_id_for_env(
            "backend",
            second_id,
            &EnvironmentSelection::Named("dev".to_string()),
        )
        .await;

    assert!(matches!(
        result,
        Err(SetCanisterIdForEnvError::CanisterIdRegister { .. })
    ));
}

#[tokio::test]
async fn test_remove_canister_id_for_env_success() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());

    // Register a canister ID
    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
    ids_store
        .register(true, "dev", "backend", canister_id)
        .unwrap();

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store.clone() as Arc<dyn IdAccess>,
        ..Context::mocked()
    };

    // Verify canister ID exists
    let lookup_result = ids_store.lookup(true, "dev", "backend").unwrap();
    assert_eq!(lookup_result, canister_id);

    // Remove the canister ID
    ctx.remove_canister_id_for_env("backend", &EnvironmentSelection::Named("dev".to_string()))
        .await
        .unwrap();

    // Verify canister ID is removed
    let lookup_result = ids_store.lookup(true, "dev", "backend");
    assert!(matches!(
        lookup_result,
        Err(crate::store_id::LookupIdError::IdNotFound { .. })
    ));
}

#[tokio::test]
async fn test_remove_canister_id_for_env_nonexistent_canister() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store.clone() as Arc<dyn IdAccess>,
        ..Context::mocked()
    };

    // Remove a canister that was never registered - should not fail
    let result = ctx
        .remove_canister_id_for_env("backend", &EnvironmentSelection::Named("dev".to_string()))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_agent_for_env_uses_environment_network() {
    let staging_root_key = vec![1, 2, 3];

    // Complex project has "test" environment which uses "staging" network
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        network: Arc::new(
            MockNetworkAccessor::new()
                .with_network(
                    "local",
                    NetworkAccess {
                        root_key: None,
                        url: Url::parse("http://localhost:8000").unwrap(),
                    },
                )
                .with_network(
                    "staging",
                    NetworkAccess {
                        root_key: Some(staging_root_key.clone()),
                        url: Url::parse("http://staging:9000").unwrap(),
                    },
                ),
        ),
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
            source: GetEnvironmentError::EnvironmentNotFound { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_env_network_not_configured() {
    // Environment "dev" exists in project and uses "local" network,
    // but "local" network is not configured in MockNetworkAccessor
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        // MockNetworkAccessor has no networks configured
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
            source: crate::network::AccessError::GetNetworkAccess { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_network_success() {
    let root_key = vec![1, 2, 3];

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        network: Arc::new(MockNetworkAccessor::new().with_network(
            "local",
            NetworkAccess {
                root_key: Some(root_key.clone()),
                url: Url::parse("http://localhost:8000").unwrap(),
            },
        )),
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
            source: GetNetworkError::NetworkNotFound { .. }
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_network_not_configured() {
    // Network "local" exists in project but is not configured in MockNetworkAccessor
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        // MockNetworkAccessor has no networks configured
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
            source: crate::network::AccessError::GetNetworkAccess { .. }
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
async fn test_get_canister_id_for_env() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());

    // Register a canister ID for the dev environment
    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
    ids_store
        .register(true, "dev", "backend", canister_id)
        .unwrap();

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store,
        ..Context::mocked()
    };

    let canister_selection = CanisterSelection::Named("backend".to_string());
    let environment_selection = EnvironmentSelection::Named("dev".to_string());

    assert!(
        matches!(ctx.get_canister_id_for_env(&canister_selection, &environment_selection).await, Ok(id) if id == canister_id)
    );

    let canister_selection = CanisterSelection::Named("INVALID".to_string());
    let environment_selection = EnvironmentSelection::Named("dev".to_string());

    let res = ctx
        .get_canister_id_for_env(&canister_selection, &environment_selection)
        .await;
    assert!(
        res.is_err(),
        "An invalid canister name should result in an error"
    );
}

#[tokio::test]
async fn test_ids_by_environment() {
    let ids_store = Arc::new(MockInMemoryIdStore::new());

    // Register multiple canister IDs for the dev environment
    let backend_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
    let frontend_id = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    ids_store
        .register(true, "dev", "backend", backend_id)
        .unwrap();
    ids_store
        .register(true, "dev", "frontend", frontend_id)
        .unwrap();

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store,
        ..Context::mocked()
    };

    let result = ctx
        .ids_by_environment(&EnvironmentSelection::Named("dev".to_string()))
        .await
        .unwrap();

    assert_eq!(result.len(), 2);
    assert_eq!(result.get("backend"), Some(&backend_id));
    assert_eq!(result.get("frontend"), Some(&frontend_id));
}

#[tokio::test]
async fn test_get_agent_defaults_outside_project() {
    let ctx = Context {
        project: Arc::new(crate::NoProjectLoader),
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
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8000),
                    },
                },
            },
        },
    };

    let mut networks = HashMap::new();
    networks.insert(
        DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        local_network.clone(),
    );

    let local_env = Environment {
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        network: local_network,
        canisters: HashMap::new(), // No canisters needed for get_agent test
    };

    let mut environments = HashMap::new();
    environments.insert(DEFAULT_LOCAL_NETWORK_NAME.to_string(), local_env);

    let project = Project {
        dir: "/project".into(),
        canisters: HashMap::new(), // No canisters needed for get_agent test
        networks,
        environments,
    };

    let ctx = Context {
        project: Arc::new(crate::MockProjectLoader::new(project)),
        network: Arc::new(MockNetworkAccessor::new().with_network(
            DEFAULT_LOCAL_NETWORK_NAME,
            NetworkAccess {
                root_key: Some(local_root_key.clone()),
                url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
            },
        )),
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
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(9000),
                    },
                },
            },
        },
    };

    let mut networks = HashMap::new();
    networks.insert(
        DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        custom_local_network.clone(),
    );

    let local_env = Environment {
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        network: custom_local_network,
        canisters: HashMap::new(), // No canisters needed for get_agent test
    };

    let mut environments = HashMap::new();
    environments.insert(DEFAULT_LOCAL_NETWORK_NAME.to_string(), local_env);

    let project = Project {
        dir: "/project".into(),
        canisters: HashMap::new(), // No canisters needed for get_agent test
        networks,
        environments,
    };

    let custom_root_key = vec![1, 2, 3, 4];

    let ctx = Context {
        project: Arc::new(crate::MockProjectLoader::new(project)),
        network: Arc::new(MockNetworkAccessor::new().with_network(
            DEFAULT_LOCAL_NETWORK_NAME,
            NetworkAccess {
                root_key: Some(custom_root_key.clone()),
                url: Url::parse("http://localhost:9000").unwrap(), // Custom port
            },
        )),
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
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8000),
                    },
                },
            },
        },
    };

    let custom_network = Network {
        name: "custom".to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(7000),
                    },
                },
            },
        },
    };

    let mut networks = HashMap::new();
    networks.insert(
        DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        default_local_network,
    );
    networks.insert("custom".to_string(), custom_network.clone());

    // "local" environment uses "custom" network
    let local_env = Environment {
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        network: custom_network,
        canisters: HashMap::new(), // No canisters needed for get_agent test
    };

    let mut environments = HashMap::new();
    environments.insert(DEFAULT_LOCAL_NETWORK_NAME.to_string(), local_env);

    let project = Project {
        dir: "/project".into(),
        canisters: HashMap::new(), // No canisters needed for get_agent test
        networks,
        environments,
    };

    let custom_root_key = vec![5, 6, 7, 8];

    let ctx = Context {
        project: Arc::new(crate::MockProjectLoader::new(project)),
        network: Arc::new(
            MockNetworkAccessor::new()
                .with_network(
                    DEFAULT_LOCAL_NETWORK_NAME,
                    NetworkAccess {
                        root_key: None,
                        url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                    },
                )
                .with_network(
                    "custom",
                    NetworkAccess {
                        root_key: Some(custom_root_key.clone()),
                        url: Url::parse("http://localhost:7000").unwrap(),
                    },
                ),
        ),
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
    let staging_root_key = vec![12, 13, 14];

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        network: Arc::new(
            MockNetworkAccessor::new()
                .with_network(
                    DEFAULT_LOCAL_NETWORK_NAME,
                    NetworkAccess {
                        root_key: None,
                        url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                    },
                )
                .with_network(
                    "staging",
                    NetworkAccess {
                        root_key: Some(staging_root_key.clone()),
                        url: Url::parse("http://localhost:8001").unwrap(),
                    },
                ),
        ),
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
    let staging_root_key = vec![15, 16, 17];

    // complex() has "test" environment using "staging" network
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        network: Arc::new(
            MockNetworkAccessor::new()
                .with_network(
                    DEFAULT_LOCAL_NETWORK_NAME,
                    NetworkAccess {
                        root_key: None,
                        url: Url::parse(DEFAULT_LOCAL_NETWORK_URL).unwrap(),
                    },
                )
                .with_network(
                    "staging",
                    NetworkAccess {
                        root_key: Some(staging_root_key.clone()),
                        url: Url::parse("http://localhost:8001").unwrap(),
                    },
                ),
        ),
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
