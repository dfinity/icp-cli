use clap::{ArgGroup, Args};
use icp::context::{EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;
use icp::project::{
    DEFAULT_LOCAL_ENVIRONMENT_NAME, DEFAULT_MAINNET_ENVIRONMENT_NAME, DEFAULT_MAINNET_NETWORK_NAME,
};
use url::Url;

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct IdentityOpt {
    /// The user identity to run this command as.
    #[arg(long, global = true)]
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
        env = "ICP_ENVIRONMENT",
        global(true),
        group = "environment-select",
        group = "network-select"
    )]
    environment: Option<String>,

    /// Shorthand for --environment=ic.
    #[arg(
        long,
        global(true),
        group = "environment-select",
        group = "network-select"
    )]
    ic: bool,
}

impl EnvironmentOpt {
    pub(crate) fn name(&self) -> &str {
        // Support --ic
        if self.ic {
            return DEFAULT_MAINNET_ENVIRONMENT_NAME;
        }

        // Otherwise, default to `local`
        self.environment
            .as_deref()
            .unwrap_or(DEFAULT_LOCAL_ENVIRONMENT_NAME)
    }
}

impl From<EnvironmentOpt> for EnvironmentSelection {
    fn from(v: EnvironmentOpt) -> Self {
        if v.ic {
            return EnvironmentSelection::Named(DEFAULT_MAINNET_ENVIRONMENT_NAME.to_string());
        }
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
    #[arg(long, env = "ICP_NETWORK", group = "network-select")]
    network: Option<String>,

    /// Shorthand for --network=mainnet
    #[arg(long, group = "network-select")]
    mainnet: bool,
}

impl From<NetworkOpt> for NetworkSelection {
    fn from(v: NetworkOpt) -> Self {
        if v.mainnet {
            return NetworkSelection::Named(DEFAULT_MAINNET_NETWORK_NAME.to_string());
        }
        match v.network {
            Some(network) => match Url::parse(&network) {
                Ok(url) => NetworkSelection::Url(url),
                Err(_) => NetworkSelection::Named(network),
            },
            None => NetworkSelection::Default,
        }
    }
}
