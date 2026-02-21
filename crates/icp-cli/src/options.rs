use clap::error::ErrorKind;
use clap::{ArgGroup, ArgMatches, Args, FromArgMatches};
use icp::context::{EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;
use icp::prelude::LOCAL;
use url::Url;

mod heading {
    pub const NETWORK_PARAMETERS: &str = "Network Selection Parameters";
    pub const IDENITTY_PARAMETERS: &str = "Identity Selection Parameters";
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct IdentityOpt {
    /// The user identity to run this command as.
    #[arg(long, global = true, help_heading = heading::IDENITTY_PARAMETERS)]
    identity: Option<String>,
}

impl From<IdentityOpt> for IdentitySelection {
    fn from(v: IdentityOpt) -> Self {
        match v.identity {
            // Anonymous
            Some(id) if id == "anonymous" => IdentitySelection::Anonymous,

            // Named
            Some(id) => IdentitySelection::Named(id),

            // Default
            None => IdentitySelection::Default,
        }
    }
}

#[derive(Args, Clone, Debug, Default)]
#[clap(group(ArgGroup::new("environment-select").multiple(false)))]
pub(crate) struct EnvironmentOpt {
    /// Override the environment to connect to. By default, the local environment is used.
    #[arg(
        long,
        short = 'e',
        env = "ICP_ENVIRONMENT",
        global(true),
        group = "environment-select",
        group = "network-select",
        help_heading = heading::NETWORK_PARAMETERS,
    )]
    environment: Option<String>,
}

impl EnvironmentOpt {
    #[allow(dead_code)]
    pub(crate) fn name(&self) -> &str {
        self.environment.as_deref().unwrap_or(LOCAL)
    }
}

impl From<EnvironmentOpt> for EnvironmentSelection {
    fn from(v: EnvironmentOpt) -> Self {
        match v.environment {
            Some(name) => EnvironmentSelection::Named(name),
            None => EnvironmentSelection::Default,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RootKey(pub Vec<u8>);

fn parse_root_key(input: &str) -> Result<RootKey, String> {
    let v = hex::decode(input).map_err(|e| format!("Invalid root key hex string: {e}"))?;
    if v.len() != 133 {
        Err(format!(
            "Invalid root key. Expected 133 bytes but got {}",
            v.len()
        ))
    } else {
        Ok(RootKey(v))
    }
}

#[derive(Clone, Debug)]
enum NetworkTarget {
    Url(Url),
    Named(String),
}

fn parse_network_target(input: &str) -> Result<NetworkTarget, String> {
    match Url::parse(input) {
        Ok(url) => Ok(NetworkTarget::Url(url)),
        Err(_) => Ok(NetworkTarget::Named(input.to_string())),
    }
}

#[derive(Args, Clone, Debug, Default)]
#[clap(group(ArgGroup::new("network-select").multiple(false)))]
pub(crate) struct NetworkOptInner {
    /// Name or URL of the network to target, conflicts with environment argument
    #[arg(long, short = 'n', env = "ICP_NETWORK", group = "network-select", help_heading = heading::NETWORK_PARAMETERS, value_parser = parse_network_target)]
    network: Option<NetworkTarget>,

    /// The root key to use if connecting to a network by URL.
    /// Required when using `--network <URL>`.
    #[arg(long, short = 'k', requires = "network", help_heading = heading::NETWORK_PARAMETERS, value_parser = parse_root_key)]
    root_key: Option<RootKey>,
}

// This is wrapper around NetworkOptInner that will do some additional
// validation to only allow --root-key when the network is a url.
#[derive(Clone, Debug, Default)]
pub(crate) enum NetworkOpt {
    Url(Url, RootKey),

    Name(String),

    #[default]
    None,
}

impl FromArgMatches for NetworkOpt {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let inner = NetworkOptInner::from_arg_matches(matches)?;

        match (inner.network, inner.root_key) {
            // Case: We have a URL, so we REQUIRE the root key
            (Some(NetworkTarget::Url(url)), Some(key)) => Ok(NetworkOpt::Url(url, key)),

            // ERROR Case: URL provided but missing root key
            (Some(NetworkTarget::Url(_)), None) => Err(clap::Error::raw(
                ErrorKind::MissingRequiredArgument,
                "`--root-key` is required when `--network` is a URL.\n",
            )),

            // Case: Named network (root key should be empty)
            (Some(NetworkTarget::Named(name)), None) => Ok(NetworkOpt::Name(name)),

            // ERROR case: Name provided with a root key
            (Some(NetworkTarget::Named(_)), Some(_)) => Err(clap::Error::raw(
                ErrorKind::MissingRequiredArgument,
                "`--root-key` is only valid when `--network` is a URL.\n",
            )),

            // Case: No network specified
            (None, None) => Ok(NetworkOpt::None),

            // Case: Should be impossible, --root-key is passed without a network argument
            (None, Some(_)) => {
                panic!("Invalid cli arg combination: --root-key without a --network <NETWORK>")
            }
        }
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        // For simple wrappers, we can just replace the current state
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for NetworkOpt {
    fn augment_args(cmd: clap::Command) -> clap::Command {
        NetworkOptInner::augment_args(cmd)
    }
    fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
        NetworkOptInner::augment_args_for_update(cmd)
    }
}

impl From<NetworkOpt> for NetworkSelection {
    fn from(v: NetworkOpt) -> Self {
        match v {
            NetworkOpt::Url(url, RootKey(key)) => NetworkSelection::Url(url, key),
            NetworkOpt::Name(name) => NetworkSelection::Named(name),
            NetworkOpt::None => NetworkSelection::Default,
        }
    }
}
