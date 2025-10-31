use std::fmt::Display;

use candid::Principal;
use clap::Args;
use ic_agent::Agent;
use icp::identity::IdentitySelection;
use snafu::Snafu;

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Snafu)]
pub(crate) enum ArgValidationError {
    #[snafu(display("You can't specify both an environment and a network"))]
    EnvironmentAndNetworkSpecified,

    #[snafu(display(
        "Specifying a network is not supported if you are targeting a canister by name, specify an environment instead"
    ))]
    AmbiguousCanisterName,

    #[snafu(transparent)]
    EnvironmentError {
        source: crate::commands::GetEnvironmentError,
    },

    #[snafu(transparent)]
    GetAgentForEnv {
        source: crate::commands::GetAgentForEnvError,
    },

    #[snafu(transparent)]
    GetCanisterIdForEnv {
        source: crate::commands::GetCanisterIdForEnvError,
    },

    #[snafu(transparent)]
    GetAgentForNetwork {
        source: crate::commands::GetAgentForNetworkError,
    },

    #[snafu(transparent)]
    GetAgentForUrl {
        source: crate::commands::GetAgentForUrlError,
    },
}

#[derive(Args, Debug)]
pub(crate) struct CanisterEnvironmentArgs {
    /// Name or principal of canister to target
    /// When using a name an environment must be specified
    pub(crate) canister: Canister,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Args, Debug)]
pub(crate) struct CanisterCommandArgs {
    // Note: Could have flattened CanisterEnvironmentArg to avoid adding child field
    /// Name or principal of canister to target
    /// When using a name an environment must be specified
    pub(crate) canister: Canister,

    #[command(flatten)]
    pub(crate) network: NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    /// The identity to use for this request
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

impl CanisterEnvironmentArgs {
    pub async fn get_cid_for_environment(
        &self,
        ctx: &Context,
    ) -> Result<Principal, ArgValidationError> {
        let arg_canister = self.canister.clone();
        let env = self.environment.clone().into();

        let principal = match arg_canister {
            Canister::Name(canister_name) => {
                ctx.get_canister_id_for_env(&canister_name, &env).await?
            }
            Canister::Principal(principal) => {
                // Make sure a valid environment was requested
                let _ = ctx.get_environment(&env).await?;
                principal
            }
        };

        Ok(principal)
    }
}

impl CanisterCommandArgs {
    pub async fn get_cid_and_agent(
        &self,
        ctx: &Context,
    ) -> Result<(Principal, Agent), ArgValidationError> {
        let arg_canister = self.canister.clone();
        let network: NetworkSelection = self.network.clone().into();
        let env: EnvironmentSelection = self.environment.clone().into();
        let identity: IdentitySelection = self.identity.clone().into();

        let (cid, agent) = match (arg_canister, env.is_explicit(), network.is_explicit()) {
            (_, true, true) => {
                // Both an environment and a network are specified this is an error
                return Err(ArgValidationError::EnvironmentAndNetworkSpecified);
            }
            (Canister::Name(_), false, true) => {
                // This is not allowed, we should not use name with an environment not a network
                return Err(ArgValidationError::AmbiguousCanisterName);
            }
            (Canister::Name(cname), _, false) => {
                // A canister name was specified so we must be in a project

                let agent = ctx.get_agent_for_env(&identity, &env).await?;
                let cid = ctx.get_canister_id_for_env(&cname, &env).await?;

                (cid, agent)
            }
            (Canister::Principal(principal), _, false) => {
                // Call by canister_id to the environment specified

                let agent = ctx.get_agent_for_env(&identity, &env).await?;

                (principal, agent)
            }
            (Canister::Principal(principal), false, true) => {
                // Should handle known networks by name

                let agent = match network {
                    NetworkSelection::Name(net_name) => {
                        ctx.get_agent_for_network(&identity, &net_name).await?
                    }
                    NetworkSelection::Url(url) => ctx.get_agent_for_url(&identity, &url).await?,
                    NetworkSelection::Default(_) => {
                        unreachable!("network is explicit but also default")
                    }
                };
                (principal, agent)
            }
        };

        Ok((cid, agent))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Canister {
    Name(String),
    Principal(Principal),
}

impl From<&str> for Canister {
    fn from(v: &str) -> Self {
        if let Ok(p) = Principal::from_text(v) {
            return Self::Principal(p);
        }

        Self::Name(v.to_string())
    }
}

impl Display for Canister {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Canister::Name(n) => f.write_str(n),
            Canister::Principal(principal) => f.write_str(&principal.to_string()),
        }
    }
}

#[derive(Args, Debug, Clone)]
pub(crate) struct NetworkOpt {
    /// Name or URL of the network to target
    #[arg(value_name = "NETWORK", long, default_value = "local")]
    pub(crate) network: String,
}

impl From<NetworkOpt> for NetworkSelection {
    fn from(v: NetworkOpt) -> Self {
        match v.network.as_str() {
            "local" => NetworkSelection::Default("local".to_string()),
            network => NetworkSelection::from(network),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NetworkSelection {
    Default(String),
    Name(String),
    Url(String),
}

impl NetworkSelection {
    pub(crate) fn is_explicit(&self) -> bool {
        matches!(self, Self::Name(_) | Self::Url(_))
    }
}

impl From<&str> for NetworkSelection {
    fn from(v: &str) -> Self {
        if v.starts_with("http://") || v.starts_with("https://") {
            return Self::Url(v.to_string());
        }

        Self::Name(v.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EnvironmentSelection {
    Name(String),
    Default(String),
}

impl EnvironmentSelection {
    pub(crate) fn name(&self) -> &str {
        match self {
            EnvironmentSelection::Name(name) => name,
            EnvironmentSelection::Default(name) => name,
        }
    }

    pub(crate) fn is_explicit(&self) -> bool {
        matches!(self, Self::Name(_))
    }
}

impl Default for EnvironmentSelection {
    fn default() -> Self {
        Self::Default("local".to_string())
    }
}

impl From<&str> for EnvironmentSelection {
    fn from(v: &str) -> Self {
        Self::Name(v.to_string())
    }
}

impl Display for EnvironmentSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use candid::Principal;

    use super::*;
    use icp::MockProjectLoader;
    use std::sync::Arc;

    use crate::store_id::MockInMemoryIdStore;

    #[test]
    fn canister_by_name() {
        assert_eq!(
            Canister::from("my-canister"),
            Canister::Name("my-canister".to_string()),
        );
    }

    #[test]
    fn canister_by_principal() {
        let cid = "ntyui-iatoh-pfi3f-27wnk-vgdqt-mq3cl-ld7jh-743kl-sde6i-tbm7g-tqe";

        assert_eq!(
            Canister::from(cid),
            Canister::Principal(Principal::from_text(cid).expect("failed to parse principal")),
        );
    }

    #[test]
    fn network_by_name() {
        assert_eq!(
            NetworkSelection::from("my-network"),
            NetworkSelection::Name("my-network".to_string()),
        );
    }

    #[test]
    fn network_by_url_http() {
        let url = "http://www.example.com";

        assert_eq!(
            NetworkSelection::from(url),
            NetworkSelection::Url("http://www.example.com".to_string()),
        );
    }

    #[tokio::test]
    async fn test_get_cid_for_environment() {
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

        let args = CanisterEnvironmentArgs {
            canister: Canister::Name("backend".to_string()),
            environment: EnvironmentOpt::with_explicit_name("dev"),
        };

        assert!(matches!(args.get_cid_for_environment(&ctx).await, Ok(id) if id == canister_id));

        let args = CanisterEnvironmentArgs {
            canister: Canister::Name("INVALID".to_string()),
            environment: EnvironmentOpt::with_explicit_name("dev"),
        };

        let res = args.get_cid_for_environment(&ctx).await;
        assert!(
            res.is_err(),
            "An invalid canister name should result in an error"
        );
    }
}
