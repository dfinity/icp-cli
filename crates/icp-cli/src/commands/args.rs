use candid::Principal;
use icp::{Network, identity::IdentitySelection};

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug)]
pub(crate) struct ArgContext {
    environment: EnvironmentOpt,
    network: Option<Network>,
    identity: IdentitySelection,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArgumentError {
    #[error("Environment and network cannot be specified at the same time.")]
    EnvironmentAndNetworkSpecified,

    #[error(
        "Specifying a network is not supported if you are targeting a canister by name. Specify an environment instead."
    )]
    CanistersAndNetworkSpecified,

    #[error("Network '{network}' not found.")]
    NetworkNotFound { network: String },

    #[error("Environment '{environment}' not found.")]
    EnvironmentNotFound { environment: String },

    #[error("Failed to load project: {0}")]
    LoadProject(#[from] icp::LoadError),
}

impl ArgContext {
    pub(crate) async fn new(
        ctx: &Context,
        environment: EnvironmentOpt,
        network: Option<Network>,
        identity: IdentityOpt,
        canisters: Vec<&str>,
    ) -> Result<Self, ArgumentError> {
        let canisters_by_name = canisters
            .iter()
            .filter(|c| Principal::from_text(c).is_err())
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        // let canisters_by_principal = canisters
        //     .iter()
        //     .filter_map(|c| Principal::from_text(c).ok())
        //     .collect::<Vec<_>>();

        if environment.is_explicit() && network.is_some() {
            return Err(ArgumentError::EnvironmentAndNetworkSpecified);
        }

        if network.is_some() && !canisters_by_name.is_empty() {
            return Err(ArgumentError::CanistersAndNetworkSpecified);
        }

        if let Some(network) = &network
            && ctx.network.access(network).await.is_err()
        {
            return Err(ArgumentError::NetworkNotFound {
                network: network.name.clone(),
            });
        }

        if environment.is_explicit()
            && !ctx
                .project
                .load()
                .await?
                .environments
                .contains_key(environment.name())
        {
            return Err(ArgumentError::EnvironmentNotFound {
                environment: environment.name().to_string(),
            });
        }

        Ok(Self {
            environment,
            network,
            identity: identity.into(),
        })
    }

    #[allow(unused)]
    pub(crate) fn network(&self) -> Option<&Network> {
        self.network.as_ref()
    }

    pub(crate) fn environment(&self) -> &str {
        self.environment.name()
    }

    pub(crate) fn identity(&self) -> &IdentitySelection {
        &self.identity
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use icp::{
        LoadError,
        network::Configuration,
        test::{MockProjectLoader, create_test_project},
    };

    use super::*;
    use crate::commands::test_utils::{TestContextBuilder, create_test_context};

    #[tokio::test]
    async fn test_succeeds_with_default_environment() {
        // Setup: Create a test project and context
        let project = create_test_project();
        let ctx = create_test_context(project);

        // Create ArgContext with default environment (local)
        let environment = EnvironmentOpt::default();
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, None, identity, vec![]).await;

        // Verify success
        assert!(result.is_ok());
        let arg_ctx = result.unwrap();
        assert_eq!(arg_ctx.environment(), "local");
        assert!(arg_ctx.network().is_none());
    }

    #[tokio::test]
    async fn test_fails_when_both_environment_and_network_specified() {
        // Setup
        let project = create_test_project();
        let ctx = create_test_context(project);

        // Create both environment and network
        let environment = EnvironmentOpt::with_environment("local");
        let network = Some(Network {
            name: "local".to_string(),
            configuration: Configuration::Managed(Default::default()),
        });
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, network, identity, vec![]).await;

        // Should fail with EnvironmentAndNetworkSpecified
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ArgumentError::EnvironmentAndNetworkSpecified
        ));
    }

    #[tokio::test]
    async fn test_fails_when_canister_names_specified_with_network() {
        // Setup
        let project = create_test_project();
        let ctx = create_test_context(project);

        let environment = EnvironmentOpt::default();
        let network = Some(Network {
            name: "local".to_string(),
            configuration: Configuration::Managed(Default::default()),
        });
        let identity = IdentityOpt::default();

        // Pass canister names (not principals)
        let canisters = vec!["my_canister", "another_canister"];

        let result = ArgContext::new(&ctx, environment, network, identity, canisters).await;

        // Should fail with CanistersAndNetworkSpecified
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ArgumentError::CanistersAndNetworkSpecified
        ));
    }

    #[tokio::test]
    async fn test_succeeds_when_network_specified_with_principals() {
        // Setup
        let project = create_test_project();
        let ctx = create_test_context(project);

        let environment = EnvironmentOpt::default();
        let network = Some(Network {
            name: "local".to_string(),
            configuration: Configuration::Managed(Default::default()),
        });
        let identity = IdentityOpt::default();

        // Pass only valid principals (not names)
        let canisters = vec!["aaaaa-aa", "rrkah-fqaaa-aaaaa-aaaaq-cai"];

        let result = ArgContext::new(&ctx, environment, network, identity, canisters).await;

        // Should succeed because all canisters are principals, not names
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fails_when_network_does_not_exist() {
        // Setup with custom network accessor that returns errors
        let project = create_test_project();

        // Use TestContextBuilder to inject a network accessor that will fail
        let network_accessor = icp::network::test::MockNetworkAccessor::new()
            .with_error("nonexistent".to_string(), "Network not found");

        let ctx = TestContextBuilder::new()
            .with_project(Arc::new(MockProjectLoader::new(project)))
            .with_network(Arc::new(network_accessor))
            .build();

        let environment = EnvironmentOpt::default();
        let network = Some(Network {
            name: "nonexistent".to_string(),
            configuration: Configuration::Managed(Default::default()),
        });
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, network, identity, vec![]).await;

        // Should fail with NetworkNotFound
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ArgumentError::NetworkNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_fails_when_explicit_environment_does_not_exist() {
        // Setup with project that only has "local" environment
        let project = create_test_project();
        let ctx = create_test_context(project);

        // Try to use "production" environment which doesn't exist
        let environment = EnvironmentOpt::with_environment("production");
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, None, identity, vec![]).await;

        // Should fail with EnvironmentNotFound
        assert!(result.is_err());
        match result.unwrap_err() {
            ArgumentError::EnvironmentNotFound { environment } => {
                assert_eq!(environment, "production");
            }
            other => panic!("Expected EnvironmentNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_fails_when_project_cannot_be_loaded() {
        // Setup with project loader that returns an error
        let project_loader = MockProjectLoader::with_error(LoadError::Manifest);

        let ctx = TestContextBuilder::new()
            .with_project(Arc::new(project_loader))
            .build();

        // Use explicit environment to trigger project load
        let environment = EnvironmentOpt::with_environment("local");
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, None, identity, vec![]).await;

        // Should fail with LoadProject error
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ArgumentError::LoadProject(_)));
    }

    #[tokio::test]
    async fn test_succeeds_when_network_specified_without_explicit_environment() {
        // Test that specifying network without explicit environment is valid
        let project = create_test_project();
        let ctx = create_test_context(project);

        let environment = EnvironmentOpt::default(); // Not explicit
        let network = Some(Network {
            name: "local".to_string(),
            configuration: Configuration::Managed(Default::default()),
        });
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, network, identity, vec![]).await;

        // Should succeed because environment is not explicit
        assert!(result.is_ok());
        let arg_ctx = result.unwrap();
        assert!(arg_ctx.network().is_some());
        assert_eq!(arg_ctx.network().unwrap().name, "local");
    }

    #[tokio::test]
    async fn test_succeeds_when_default_environment_does_not_exist() {
        // Test that default (non-explicit) environment doesn't trigger validation
        // even if it doesn't exist in project
        let project = create_test_project(); // Only has "local" environment
        let ctx = create_test_context(project);

        // Use default which would fail if validated, but shouldn't be validated
        let environment = EnvironmentOpt::default(); // is_explicit() == false
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, None, identity, vec![]).await;

        // Should succeed because default environments are not validated
        assert!(result.is_ok());
        assert_eq!(result.unwrap().environment(), "local");
    }

    #[tokio::test]
    async fn test_succeeds_when_ic_flag_maps_to_ic_environment() {
        // Setup - need to add "ic" environment to the project
        let mut project = create_test_project();

        let ic_network = Network {
            name: "ic".to_string(),
            configuration: Configuration::Managed(Default::default()),
        };

        project
            .networks
            .insert("ic".to_string(), ic_network.clone());
        project.environments.insert(
            "ic".to_string(),
            icp::Environment {
                name: "ic".to_string(),
                network: ic_network,
                canisters: Default::default(),
            },
        );

        let ctx = create_test_context(project);

        // Use --ic flag shorthand
        let environment = EnvironmentOpt::with_ic();
        let identity = IdentityOpt::default();

        let result = ArgContext::new(&ctx, environment, None, identity, vec![]).await;

        assert!(result.is_ok());
        let arg_ctx = result.unwrap();
        assert_eq!(arg_ctx.environment(), "ic");
    }
}
