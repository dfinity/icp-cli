use clap::{ArgGroup, Args};
use icp::context::{EnvironmentSelection, IC_ROOT_KEY, NetworkSelection};
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
struct RootKey(pub Vec<u8>);

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
pub(crate) struct NetworkOpt {
    /// Name or URL of the network to target, conflicts with environment argument
    #[arg(long, short = 'n', env = "ICP_NETWORK", group = "network-select", help_heading = heading::NETWORK_PARAMETERS, value_parser = parse_network_target)]
    network: Option<NetworkTarget>,

    /// The root key to use if connecting to a network by URL.
    #[arg(long, short = 'k', requires = "network", help_heading = heading::NETWORK_PARAMETERS, value_parser = parse_root_key)]
    root_key: Option<RootKey>,
}

impl From<NetworkOpt> for NetworkSelection {
    fn from(v: NetworkOpt) -> Self {
        match v.network {
            Some(target) => match target {
                NetworkTarget::Url(url) => {
                    let root_key = match v.root_key {
                        Some(RootKey(k)) => k,
                        None => IC_ROOT_KEY.to_vec(),
                    };
                    NetworkSelection::Url(url, root_key)
                }
                NetworkTarget::Named(name) => NetworkSelection::Named(name),
            },
            None => NetworkSelection::Default,
        }
    }
}
