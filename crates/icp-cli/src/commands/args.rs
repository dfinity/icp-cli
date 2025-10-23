use std::fmt::Display;

use candid::Principal;
use clap::Args;
use ic_agent::Agent;
use icp::identity::IdentitySelection;

use crate::{commands::Context, options::IdentityOpt};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArgValidationError {
    #[error("You can't specify both an environment and a network")]
    EnvironmentAndNetworkSpecified,

    #[error(
        "Specifying a network is not supported if you are targeting a canister by name, specify an environment instead"
    )]
    AmbiguousCanisterName,

    #[error(transparent)]
    ProjectLoad(#[from] icp::LoadError),

    #[error(transparent)]
    Lookup(#[from] crate::store_id::LookupError),

    #[error(transparent)]
    Access(#[from] icp::network::AccessError),

    #[error(transparent)]
    Agent(#[from] icp::agent::CreateError),

    #[error(transparent)]
    Identity(#[from] icp::identity::LoadError),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

#[derive(Args, Debug)]
pub(crate) struct CanisterCommandArgs {
    /// Name of canister to target
    pub(crate) canister: Canister,

    #[arg(long)]
    pub(crate) network: Option<Network>,

    #[arg(long)]
    pub(crate) environment: Option<Environment>,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

impl CanisterCommandArgs {
    pub async fn get_cid_and_agent(
        &self,
        ctx: &Context,
    ) -> Result<(Principal, Agent), ArgValidationError> {
        let arg_canister = self.canister.clone();
        let arg_environment = self.environment.clone().unwrap_or_default();
        let env_name = arg_environment.name();
        let arg_network = self.network.clone();
        let identity_selection: IdentitySelection = self.identity.clone().into();

        let (cid, agent) = match (arg_canister, &arg_environment, arg_network) {
            (_, Environment::Name(_), Some(_)) => {
                // Both an environment and a network are specified this is an error
                return Err(ArgValidationError::EnvironmentAndNetworkSpecified);
            }
            (Canister::Name(_), Environment::Default(_), Some(_)) => {
                // This is not allowed, we should not use name with an environment not a network
                return Err(ArgValidationError::AmbiguousCanisterName);
            }
            (Canister::Name(cname), _, None) => {
                // A canister name was specified so we must be in a project

                let agent = ctx
                    .get_agent_for_env(&identity_selection, env_name)
                    .await?;
                let cid = ctx.get_canister_id_for_env(&cname, env_name).await?;

                (cid, agent)
            }
            (Canister::Principal(principal), _, None) => {
                // Call by canister_id to the environment specified

                let agent = ctx.get_agent_for_env(&identity_selection, env_name).await?;

                (principal, agent)
            }
            (Canister::Principal(principal), Environment::Default(_), Some(network)) => {
                // Should handle known networks by name

                let agent = match network {
                    Network::Name(net_name) => {
                        ctx.get_agent_for_network(&identity_selection, &net_name)
                            .await?
                    }
                    Network::Url(url) => ctx.get_agent_for_url(&identity_selection, &url).await?,
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Network {
    Name(String),
    Url(String),
}

impl From<&str> for Network {
    fn from(v: &str) -> Self {
        if v.starts_with("http://") || v.starts_with("https://") {
            return Self::Url(v.to_string());
        }

        Self::Name(v.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Environment {
    Name(String),
    Default(String),
}

impl Environment {
    pub(crate) fn name(&self) -> &str {
        match self {
            Environment::Name(name) => name,
            Environment::Default(name) => name,
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::Default("local".to_string())
    }
}

impl From<&str> for Environment {
    fn from(v: &str) -> Self {
        Self::Name(v.to_string())
    }
}

impl Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use candid::Principal;

    use crate::commands::args::{Canister, Network};

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
            Network::from("my-network"),
            Network::Name("my-network".to_string()),
        );
    }

    #[test]
    fn network_by_url_http() {
        let url = "http://www.example.com";

        assert_eq!(
            Network::from(url),
            Network::Url("http://www.example.com".to_string()),
        );
    }
}
