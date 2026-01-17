use clap::Args;
use icp::{context::NetworkOrEnvironmentSelection, prelude::LOCAL};

#[derive(Args, Clone, Debug)]
pub(crate) struct NetworkOrEnvironmentArgs {
    /// Name of the network to use
    #[arg(
        help = "Name of the network to use. Overrides ICP_ENVIRONMENT if set.",
        long_help = "Name of the network to use.\n\n\
                     Takes precedence over -e/--environment and the ICP_ENVIRONMENT \
                     environment variable when specified explicitly."
    )]
    pub(crate) name: Option<String>,

    /// Use the network from the specified environment
    #[arg(
        short = 'e',
        long,
        help = "Use the network from the specified environment",
        long_help = "Use the network configured in the specified environment.\n\n\
                     Cannot be used together with an explicit network name argument.\n\
                     The ICP_ENVIRONMENT environment variable is also checked when \
                     neither network name nor -e flag is specified."
    )]
    pub(crate) environment: Option<String>,
}

impl From<NetworkOrEnvironmentArgs> for Result<NetworkOrEnvironmentSelection, anyhow::Error> {
    fn from(args: NetworkOrEnvironmentArgs) -> Self {
        // Check for mutual exclusivity (both explicit)
        if args.name.is_some() && args.environment.is_some() {
            return Err(anyhow::anyhow!(
                "Cannot specify both network name and environment. \
                 Use either a network name or -e/--environment, not both."
            ));
        }

        // Precedence 1: Explicit network name (highest)
        if let Some(name) = args.name {
            return Ok(NetworkOrEnvironmentSelection::Network(name));
        }

        // Precedence 2: Explicit environment flag
        if let Some(env_name) = args.environment {
            return Ok(NetworkOrEnvironmentSelection::Environment(env_name));
        }

        // Precedence 3: ICP_ENVIRONMENT variable
        if let Ok(env_name) = std::env::var("ICP_ENVIRONMENT") {
            return Ok(NetworkOrEnvironmentSelection::Environment(env_name));
        }

        // Precedence 4: Default to "local" network (lowest)
        Ok(NetworkOrEnvironmentSelection::Network(LOCAL.to_string()))
    }
}
