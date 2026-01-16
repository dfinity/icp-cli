use clap::{ArgGroup, Args};
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

#[derive(Args, Clone, Debug, Default)]
#[clap(group(ArgGroup::new("network-select").multiple(false)))]
pub(crate) struct NetworkOpt {
    /// Name of the network to target, conflicts with environment argument
    #[arg(long, short = 'n', env = "ICP_NETWORK", group = "network-select", help_heading = heading::NETWORK_PARAMETERS)]
    network: Option<String>,
}

impl From<NetworkOpt> for NetworkSelection {
    fn from(v: NetworkOpt) -> Self {
        match v.network {
            Some(network) => match Url::parse(&network) {
                Ok(url) => NetworkSelection::Url(url),
                Err(_) => NetworkSelection::Named(network),
            },
            None => NetworkSelection::Default,
        }
    }
}
