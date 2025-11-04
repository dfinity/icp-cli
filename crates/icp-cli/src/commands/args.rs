use std::fmt::Display;

use candid::Principal;
use clap::Args;
use icp::context::{CanisterSelection, EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;

use crate::options::{EnvironmentOpt, IdentityOpt};

#[derive(Args, Debug)]
pub(crate) struct CanisterEnvironmentArgs {
    /// Name or principal of canister to target
    /// When using a name an environment must be specified
    pub(crate) canister: Canister,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

impl CanisterEnvironmentArgs {
    /// Convert arguments into selection enums for canister and environment
    pub(crate) fn selections(&self) -> (CanisterSelection, EnvironmentSelection) {
        let canister_selection: CanisterSelection = self.canister.clone().into();
        let environment_selection: EnvironmentSelection = self.environment.clone().into();
        (canister_selection, environment_selection)
    }
}

#[derive(Args, Debug)]
pub(crate) struct CanisterCommandArgs {
    // Note: Could have flattened CanisterEnvironmentArg to avoid adding child field
    /// Name or principal of canister to target
    /// When using a name an environment must be specified
    pub(crate) canister: Canister,

    /// Name of the network to target, conflicts with environment argument
    #[arg(long, conflicts_with = "environment")]
    pub(crate) network: Option<Network>,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    /// The identity to use for this request
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

/// Selections derived from CanisterCommandArgs
pub(crate) struct CommandSelections {
    pub(crate) canister: CanisterSelection,
    pub(crate) environment: EnvironmentSelection,
    pub(crate) network: NetworkSelection,
    pub(crate) identity: IdentitySelection,
}

impl CanisterCommandArgs {
    /// Convert command arguments into selection enums
    pub(crate) fn selections(&self) -> CommandSelections {
        let canister_selection: CanisterSelection = self.canister.clone().into();
        let environment_selection: EnvironmentSelection = self.environment.clone().into();
        let network_selection: NetworkSelection = match self.network.clone() {
            Some(network) => network.into_selection(),
            None => NetworkSelection::FromEnvironment,
        };
        let identity_selection: IdentitySelection = self.identity.clone().into();

        CommandSelections {
            canister: canister_selection,
            environment: environment_selection,
            network: network_selection,
            identity: identity_selection,
        }
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

impl From<Canister> for CanisterSelection {
    fn from(v: Canister) -> Self {
        match v {
            Canister::Name(name) => CanisterSelection::Named(name),
            Canister::Principal(principal) => CanisterSelection::Principal(principal),
        }
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

impl Network {
    pub(crate) fn into_selection(self) -> NetworkSelection {
        match self {
            Network::Name(name) => NetworkSelection::Named(name),
            Network::Url(url) => NetworkSelection::Url(url),
        }
    }
}

#[cfg(test)]
mod tests {
    use candid::Principal;

    use super::*;

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
