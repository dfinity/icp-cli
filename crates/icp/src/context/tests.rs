use super::*;
use crate::{
    MockProjectLoader,
    store_id::{Access as IdAccess, mock::MockInMemoryIdStore},
};
use candid::Principal;

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
        .get_network(&NetworkSelection::Named(LOCAL.to_string()))
        .await
        .unwrap();

    assert_eq!(network.name, LOCAL);
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
