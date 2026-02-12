use std::fmt::Display;
use std::str::FromStr;

use candid::Principal;
use clap::Args;
use ic_ledger_types::AccountIdentifier;
use icp::context::{CanisterSelection, EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;
use icrc_ledger_types::icrc1::account::Account;

use crate::options::{EnvironmentOpt, IdentityOpt, NetworkOpt};

#[derive(Args, Debug)]
pub(crate) struct CanisterCommandArgs {
    // Note: Could have flattened CanisterEnvironmentArg to avoid adding child field
    /// Name or principal of canister to target
    /// When using a name an environment must be specified.
    pub(crate) canister: Canister,

    #[command(flatten)]
    pub(crate) network: NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

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
        let network_selection: NetworkSelection = self.network.clone().into();
        let identity_selection: IdentitySelection = self.identity.clone().into();

        CommandSelections {
            canister: canister_selection,
            environment: environment_selection,
            network: network_selection,
            identity: identity_selection,
        }
    }
}

// Common argument used for Token and Cycles commands
#[derive(Args, Clone, Debug)]
pub(crate) struct TokenCommandArgs {
    #[command(flatten)]
    pub(crate) network: NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

/// Selections derived from TokenCommandArgs
pub(crate) struct TokenCommandSelections {
    pub(crate) environment: EnvironmentSelection,
    pub(crate) network: NetworkSelection,
    pub(crate) identity: IdentitySelection,
}

impl TokenCommandArgs {
    /// Convert command arguments into selection enums
    pub(crate) fn selections(&self) -> TokenCommandSelections {
        let environment_selection: EnvironmentSelection = self.environment.clone().into();
        let network_selection: NetworkSelection = self.network.clone().into();
        let identity_selection: IdentitySelection = self.identity.clone().into();

        TokenCommandSelections {
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum FlexibleAccountId {
    Icrc1(Account),
    IcpLedger(AccountIdentifier),
}

impl FromStr for FlexibleAccountId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try parsing as ICP ledger account (hex string)
        if let Ok(bytes) = hex::decode(s) {
            if bytes.len() == 32 {
                let mut array = [0u8; 32];
                array.copy_from_slice(&bytes);
                return Ok(FlexibleAccountId::IcpLedger(
                    AccountIdentifier::from_slice(&array).unwrap(),
                ));
            } else {
                return Err(format!("Invalid ICP ledger account hex string: {s}"));
            }
        }
        // Try parsing as ICRC1 account
        if let Ok(account) = s.parse::<Account>() {
            return Ok(FlexibleAccountId::Icrc1(account));
        }

        Err(format!("Invalid principal / account identifier: {s}"))
    }
}

impl Display for FlexibleAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlexibleAccountId::Icrc1(account) => account.fmt(f),
            FlexibleAccountId::IcpLedger(bytes) => hex::encode(bytes).fmt(f),
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
}
