use super::*;
use crate::store_id::MockInMemoryIdStore;
use crate::{MockProjectLoader, identity::MockIdentityLoader, network::MockNetworkAccessor};

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
            source: crate::identity::LoadError::LoadIdentity(_)
        })
    ));
}

#[tokio::test]
async fn test_get_environment_success() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    let env = ctx.get_environment("dev").await.unwrap();

    assert_eq!(env.name, "dev");
}

#[tokio::test]
async fn test_get_environment_not_found() {
    let ctx = Context::mocked();

    let result = ctx.get_environment("nonexistent").await;

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

    let network = ctx.get_network("local").await.unwrap();

    assert_eq!(network.name, "local");
}

#[tokio::test]
async fn test_get_network_not_found() {
    let ctx = Context::mocked();

    let result = ctx.get_network("nonexistent").await;

    assert!(matches!(
        result,
        Err(GetNetworkError::NetworkNotFound { ref name }) if name == "nonexistent"
    ));
}

#[tokio::test]
async fn test_get_canister_id_for_env_success() {
    use crate::store_id::{Access as IdAccess, Key};
    use candid::Principal;

    let ids_store = Arc::new(MockInMemoryIdStore::new());

    // Register a canister ID for the dev environment
    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
    ids_store
        .register(
            &Key {
                network: "local".to_string(),
                environment: "dev".to_string(),
                canister: "backend".to_string(),
            },
            &canister_id,
        )
        .unwrap();

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ids: ids_store,
        ..Context::mocked()
    };

    let cid = ctx.get_canister_id_for_env("backend", "dev").await.unwrap();

    assert_eq!(cid, canister_id);
}

#[tokio::test]
async fn test_get_canister_id_for_env_canister_not_in_env() {
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        ..Context::mocked()
    };

    // "database" is only in "dev" environment, not in "test"
    let result = ctx.get_canister_id_for_env("database", "test").await;

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
    let result = ctx.get_canister_id_for_env("backend", "dev").await;

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
async fn test_get_agent_for_env_uses_environment_network() {
    use crate::network::access::NetworkAccess;

    let staging_root_key = vec![1, 2, 3];

    // Complex project has "test" environment which uses "staging" network
    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        network: Arc::new(
            MockNetworkAccessor::new()
                .with_network(
                    "local",
                    NetworkAccess {
                        default_effective_canister_id: None,
                        root_key: None,
                        url: "http://localhost:8000".to_string(),
                    },
                )
                .with_network(
                    "staging",
                    NetworkAccess {
                        default_effective_canister_id: None,
                        root_key: Some(staging_root_key.clone()),
                        url: "http://staging:9000".to_string(),
                    },
                ),
        ),
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent_for_env(&IdentitySelection::Anonymous, "test")
        .await
        .unwrap();

    assert_eq!(agent.read_root_key(), staging_root_key);
}

#[tokio::test]
async fn test_get_agent_for_env_environment_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_agent_for_env(&IdentitySelection::Anonymous, "nonexistent")
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
        .get_agent_for_env(&IdentitySelection::Anonymous, "dev")
        .await;

    assert!(matches!(
        result,
        Err(GetAgentForEnvError::NetworkAccess {
            source: crate::network::AccessError::Unexpected(_)
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_network_success() {
    use crate::network::access::NetworkAccess;

    let root_key = vec![1, 2, 3];

    let ctx = Context {
        project: Arc::new(MockProjectLoader::complex()),
        network: Arc::new(MockNetworkAccessor::new().with_network(
            "local",
            NetworkAccess {
                default_effective_canister_id: None,
                root_key: Some(root_key.clone()),
                url: "http://localhost:8000".to_string(),
            },
        )),
        ..Context::mocked()
    };

    let agent = ctx
        .get_agent_for_network(&IdentitySelection::Anonymous, "local")
        .await
        .unwrap();

    assert_eq!(agent.read_root_key(), root_key);
}

#[tokio::test]
async fn test_get_agent_for_network_network_not_found() {
    let ctx = Context::mocked();

    let result = ctx
        .get_agent_for_network(&IdentitySelection::Anonymous, "nonexistent")
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
        .get_agent_for_network(&IdentitySelection::Anonymous, "local")
        .await;

    assert!(matches!(
        result,
        Err(GetAgentForNetworkError::NetworkAccess {
            source: crate::network::AccessError::Unexpected(_)
        })
    ));
}

#[tokio::test]
async fn test_get_agent_for_url_success() {
    let ctx = Context::mocked();

    let result = ctx
        .get_agent_for_url(&IdentitySelection::Anonymous, "http://localhost:8000")
        .await;

    assert!(result.is_ok());
}

