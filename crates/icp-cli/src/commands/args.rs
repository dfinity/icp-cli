use std::fmt::Display;

use candid::Principal;
use clap::Args;

use crate::options::IdentityOpt;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArgValidationError {
    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error("project does not contain a network named '{name}'")]
    NetworkNotFound { name: String },

    #[error("environment '{environment}' does not include canister '{canister}'")]
    CanisterNotInEnvironment {
        environment: String,
        canister: String,
    },

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
}

#[derive(Args, Debug)]
pub(crate) struct CanisterCommandArgs {
    /// Name of canister to call to
    pub(crate) canister: Canister,

    #[arg(long)]
    pub(crate) network: Option<Network>,

    #[arg(long, default_value_t = Environment::default())]
    pub(crate) environment: Environment,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
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
        write!(
            f,
            "{}",
            match self {
                Environment::Name(name) => name.to_string(),
                Environment::Default(name) => name.to_string(),
            }
        )
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
