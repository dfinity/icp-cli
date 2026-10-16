use std::fmt::Display;

use anyhow::{Context as _, bail};
use candid::Principal;
use clap::{Args, ValueHint};
use clap_complete::ArgValueCandidates;
use icp_app::context::NetworkSelection;
use icp_app::identity::IdentitySelection;
use icp_project::host::{CanisterSelection, EnvironmentSelection};
use icp_project::manifest::ArgsFormat;
use icp_project::prelude::PathBuf;
use icp_project::{CanisterArgs, fs};

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

/// Flags for a command printed to be pasted.
pub(crate) struct PasteFlags {
    /// Each flag with a leading space.
    pub(crate) text: String,
    /// The placeholders in `text`, such as `<your identity>`, standing for
    /// values no quoting writes safely for every shell (see [`shell_word`]).
    pub(crate) blanks: Vec<&'static str>,
}

impl PasteFlags {
    /// A line for each placeholder, asking to fill it in, to print after the
    /// command it is in.
    pub(crate) fn notes(&self) -> Vec<String> {
        self.blanks
            .iter()
            .map(|blank| {
                format!("Fill in {blank} yourself: no quoting of it works in every shell.")
            })
            .collect()
    }
}

/// The flags that select `identity`, `network` and `environment` again, for
/// a command printed to be pasted: the short form where there is one, and
/// `--flag=value` for a value starting with `-`, which would read as a flag.
pub(crate) fn selection_flags(
    identity: &IdentitySelection,
    network: &NetworkSelection,
    environment: &EnvironmentSelection,
) -> PasteFlags {
    let mut flags = PasteFlags {
        text: String::new(),
        blanks: Vec::new(),
    };
    let mut push = |short: &str, long: &str, value: &str, blank: &'static str| {
        let word = if value.starts_with('-') {
            shell_word(&format!("{long}={value}"))
        } else {
            shell_word(value).map(|value| format!("{short} {value}"))
        };
        match word {
            Some(word) => flags.text.push_str(&format!(" {word}")),
            None => {
                flags.text.push_str(&format!(" {short} {blank}"));
                flags.blanks.push(blank);
            }
        }
    };
    match identity {
        IdentitySelection::Default => {}
        IdentitySelection::Anonymous => {
            push("--identity", "--identity", "anonymous", "<your identity>");
        }
        IdentitySelection::Named(name) => {
            push("--identity", "--identity", name, "<your identity>");
        }
    }
    match network {
        NetworkSelection::Default => {}
        NetworkSelection::Named(name) => push("-n", "--network", name, "<your network>"),
        NetworkSelection::Url(url, root_key) => {
            push("-n", "--network", url.as_str(), "<your network>");
            push(
                "-k",
                "--root-key",
                &String::from(root_key.clone()),
                "<your root key>",
            );
        }
    }
    if let EnvironmentSelection::Named(name) = environment {
        push("-e", "--environment", name, "<your environment>");
    }
    flags
}

/// `value` as one word that reads back as it is in every shell icp-cli
/// completes for (the POSIX shells, fish, elvish and PowerShell): bare when
/// nothing in it is special to any of them, else in single quotes, else, for
/// a value with a single quote in it, in double quotes when nothing else in
/// it is special there. `None` when none of those will do.
pub(crate) fn shell_word(value: &str) -> Option<String> {
    // Control characters do not paste, and PowerShell ends a quoted string
    // at any of its curly quotes.
    if value
        .chars()
        .any(|c| c.is_control() || ('\u{2018}'..='\u{201E}').contains(&c))
    {
        return None;
    }
    // zsh reads a leading `=` as a path lookup, and PowerShell a leading `@`
    // as splatting.
    let bare = !value.is_empty()
        && !value.starts_with('=')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=+".contains(c));
    if bare {
        Some(value.to_owned())
    } else if !value.contains(['\'', '\\']) {
        // fish reads `\\` and `\'` even between single quotes.
        Some(format!("'{value}'"))
    } else if !value.contains(['\\', '"', '$', '`', '!']) {
        Some(format!("\"{value}\""))
    } else {
        None
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

pub(crate) use icp_project::parsers::FlexibleAccountId;

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
    fn words_are_quoted_only_as_every_shell_needs() {
        for (value, word) in [
            ("alice", Some("alice")),
            ("my-id_2.dev:x+y", Some("my-id_2.dev:x+y")),
            ("http://localhost:8000/", Some("http://localhost:8000/")),
            ("--identity=-dash", Some("--identity=-dash")),
            ("my id", Some("'my id'")),
            ("@team", Some("'@team'")),
            ("=eq", Some("'=eq'")),
            ("a$b", Some("'a$b'")),
            ("a!b", Some("'a!b'")),
            ("naïve", Some("'naïve'")),
            ("", Some("''")),
            ("o'neil", Some("\"o'neil\"")),
            ("o'neil $x", None),
            ("o'neil!", None),
            ("a\\b", None),
            ("tab\there", None),
            ("o\u{2019}neil", None),
        ] {
            assert_eq!(shell_word(value).as_deref(), word, "{value}");
        }
    }

    #[test]
    fn selections_become_the_flags_that_select_them() {
        let flags = |identity, network, environment: EnvironmentSelection| {
            let flags = selection_flags(&identity, &network, &environment);
            (flags.text, flags.blanks)
        };
        let named = |name: &str| IdentitySelection::Named(name.into());
        let env = |name: &str| EnvironmentSelection::Named(name.into());
        let none: Vec<&str> = vec![];
        assert_eq!(
            flags(
                IdentitySelection::Default,
                NetworkSelection::Default,
                EnvironmentSelection::Default
            ),
            (String::new(), none.clone())
        );
        assert_eq!(
            flags(named("alice"), NetworkSelection::Default, env("ic")),
            (" --identity alice -e ic".into(), none.clone())
        );
        assert_eq!(
            flags(
                IdentitySelection::Anonymous,
                NetworkSelection::Named("ic".into()),
                EnvironmentSelection::Default
            ),
            (" --identity anonymous -n ic".into(), none.clone())
        );
        assert_eq!(
            flags(
                named("-dash"),
                NetworkSelection::Named("-net".into()),
                env("-env")
            ),
            (
                " --identity=-dash --network=-net --environment=-env".into(),
                none.clone()
            )
        );
        assert_eq!(
            flags(named("-my id"), NetworkSelection::Default, env("ic")),
            (" '--identity=-my id' -e ic".into(), none.clone())
        );
        assert_eq!(
            flags(
                named("o'neil"),
                NetworkSelection::Url(
                    "http://localhost:8000".parse().unwrap(),
                    icp_project::network::RootKeySpec::Fetch
                ),
                EnvironmentSelection::Default
            ),
            (
                " --identity \"o'neil\" -n http://localhost:8000/ -k fetch".into(),
                none.clone()
            )
        );
        assert_eq!(
            flags(named("o'neil $x"), NetworkSelection::Default, env("ic")),
            (
                " --identity <your identity> -e ic".into(),
                vec!["<your identity>"]
            )
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
