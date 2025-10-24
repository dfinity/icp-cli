use candid::Principal;
use icp::{Network, identity::IdentitySelection};

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
};

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

        if let Some(network) = &network {
            if ctx.network.access(network).await.is_err() {
                return Err(ArgumentError::NetworkNotFound {
                    network: network.name.clone(),
                });
            }
        }

        if environment.is_explicit() {
            if ctx
                .project
                .load()
                .await?
                .environments
                .get(environment.name())
                .is_none()
            {
                return Err(ArgumentError::EnvironmentNotFound {
                    environment: environment.name().to_string(),
                });
            }
        }

        let environment = environment;
        let identity = identity.into();
        Ok(Self {
            environment,
            network,
            identity,
        })
    }

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
