use clap::{ArgGroup, Args};
use icp_project::{ENVIRONMENT_IC, ENVIRONMENT_LOCAL};

#[derive(Args, Clone, Debug, Default)]
pub struct IdentityOpt {
    /// The user identity to run this command as.
    #[arg(long, global = true)]
    identity: Option<String>,
}

impl IdentityOpt {
    pub fn name(&self) -> Option<&str> {
        self.identity.as_deref()
    }
}

#[derive(Args, Clone, Debug, Default)]
#[clap(group(ArgGroup::new("environment-select").multiple(false)))]
pub struct EnvironmentOpt {
    /// Override the environment to connect to. By default, the local environment is used.
    #[arg(
        long,
        env = "ICP_ENVIRONMENT",
        global(true),
        group = "environment-select"
    )]
    environment: Option<String>,

    /// Shorthand for --environment=ic.
    #[clap(long, global(true), group = "environment-select")]
    ic: bool,
}

impl EnvironmentOpt {
    pub fn name(&self) -> &str {
        // Support --ic
        if self.ic {
            return ENVIRONMENT_IC;
        }

        // Otherwise, default to `local`
        self.environment.as_deref().unwrap_or(ENVIRONMENT_LOCAL)
    }

    pub fn is_mainnet(&self) -> bool {
        self.name() == ENVIRONMENT_IC
    }

    /// Refers to `aaaaa-aa:provisional_create_canister_with_cycles` and `aaaaa-aa:provisional_top_up_canister`
    pub fn supports_provisional_api(&self) -> bool {
        // Provisional API is not supported on mainnet. In the future, maybe it is also not supported on e.g. UTOPIA networks.
        !self.is_mainnet()
    }
}
