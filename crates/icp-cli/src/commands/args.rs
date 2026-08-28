use std::fmt::Display;

use anyhow::{Context as _, bail};
use candid::Principal;
use clap::{Args, ValueHint};
use clap_complete::ArgValueCandidates;
use icp::context::{CanisterSelection, EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;
use icp::manifest::ArgsFormat;
use icp::prelude::PathBuf;
use icp::{CanisterArgs, fs};

use crate::options::{EnvironmentOpt, IdentityOpt, NetworkOpt};

/// Selections derived from CanisterCommandArgs
pub(crate) struct CommandSelections {
    pub(crate) canister: CanisterSelection,
    pub(crate) environment: EnvironmentSelection,
    pub(crate) network: NetworkSelection,
    pub(crate) identity: IdentitySelection,
}

#[derive(Args, Debug)]
pub(crate) struct CanisterCommandArgs {
    // Note: Could have flattened CanisterEnvironmentArg to avoid adding child field
    /// Name or principal of canister to target.
    /// When using a name an environment must be specified.
    #[arg(add = ArgValueCandidates::new(crate::complete::canisters))]
    pub(crate) canister: Canister,

    #[command(flatten)]
    pub(crate) network: NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
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

/// Selections derived from OptionalCanisterCommandArgs
pub(crate) struct OptionalCanisterCommandSelections {
    pub(crate) canister: Option<CanisterSelection>,
    pub(crate) environment: EnvironmentSelection,
    pub(crate) network: NetworkSelection,
    pub(crate) identity: IdentitySelection,
}

impl OptionalCanisterCommandArgs {
    /// Convert command arguments into selection enums
    pub(crate) fn selections(&self) -> OptionalCanisterCommandSelections {
        let canister_selection: Option<CanisterSelection> =
            self.canister.as_ref().map(|c| c.clone().into());
        let environment_selection: EnvironmentSelection = self.environment.clone().into();
        let network_selection: NetworkSelection = self.network.clone().into();
        let identity_selection: IdentitySelection = self.identity.clone().into();

        OptionalCanisterCommandSelections {
            canister: canister_selection,
            environment: environment_selection,
            network: network_selection,
            identity: identity_selection,
        }
    }
}

// Like the CanisterCommandArgs but canister is optional
#[derive(Args, Debug)]
pub(crate) struct OptionalCanisterCommandArgs {
    /// Name or principal of canister to target.
    /// When using a name an environment must be specified.
    #[arg(add = ArgValueCandidates::new(crate::complete::canisters))]
    pub(crate) canister: Option<Canister>,

    #[command(flatten)]
    pub(crate) network: NetworkOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
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

pub(crate) use icp::parsers::FlexibleAccountId;

/// Grouped flags for specifying canister install arguments, shared by `canister install`, and `deploy`.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ArgsOpt {
    /// Inline arguments, interpreted per `--args-format` (Candid by default).
    #[arg(long, conflicts_with = "args_file")]
    pub(crate) args: Option<String>,

    /// Path to a file containing arguments.
    #[arg(long, conflicts_with = "args", value_hint = ValueHint::FilePath)]
    pub(crate) args_file: Option<PathBuf>,

    /// Format of the arguments.
    #[arg(long, default_value = "candid")]
    pub(crate) args_format: ArgsFormat,
}

impl ArgsOpt {
    /// Returns whether any args were provided via CLI flags.
    pub(crate) fn is_some(&self) -> bool {
        self.args.is_some() || self.args_file.is_some()
    }

    /// Resolve CLI args to raw bytes, reading files as needed.
    /// Returns `None` if no args were provided.
    pub(crate) fn resolve_bytes(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        load_args(
            self.args.as_deref(),
            self.args_file.as_ref(),
            &self.args_format,
            "--args",
        )?
        .as_ref()
        .map(|ia| ia.to_bytes().context("failed to encode args"))
        .transpose()
    }
}

/// Load args from an inline value or a file, returning the intermediate [`CanisterArgs`]
/// representation. Returns `None` if neither was provided.
///
/// `inline_arg_name` is used in the error message when `--args-format bin` is given
/// with an inline value (e.g. `"--args"` or `"a positional argument"`).
pub(crate) fn load_args(
    inline_value: Option<&str>,
    args_file: Option<&PathBuf>,
    args_format: &ArgsFormat,
    inline_arg_name: &str,
) -> Result<Option<CanisterArgs>, anyhow::Error> {
    match (inline_value, args_file) {
        (Some(value), None) => {
            if *args_format == ArgsFormat::Bin {
                bail!("--args-format bin requires --args-file, not {inline_arg_name}");
            }
            Ok(Some(CanisterArgs::Text {
                content: value.to_owned(),
                format: args_format.clone(),
            }))
        }
        (None, Some(file_path)) => Ok(Some(match args_format {
            ArgsFormat::Bin => {
                let bytes = fs::read(file_path).context("failed to read args file")?;
                CanisterArgs::Binary(bytes)
            }
            fmt => {
                let content = fs::read_to_string(file_path).context("failed to read args file")?;
                CanisterArgs::Text {
                    content: content.trim().to_owned(),
                    format: fmt.clone(),
                }
            }
        })),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
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
