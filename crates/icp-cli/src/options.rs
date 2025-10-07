use clap::{ArgGroup, Args};
use icp::identity::IdentitySelection;

#[derive(Args, Clone, Debug, Default)]
pub struct IdentityOpt {
    /// The user identity to run this command as.
    #[arg(long, global = true)]
    identity: Option<String>,
}

impl From<IdentityOpt> for IdentitySelection {
    fn from(v: IdentityOpt) -> Self {
        match v.identity {
            // Anonymous
            Some(id) if id.to_string() == "anonymous" => IdentitySelection::Anonymous,

            // Named
            Some(id) => IdentitySelection::Named(id),

            // Default
            None => IdentitySelection::Default,
        }
    }
}

// impl IdentityOpt {
//     pub fn name(&self) -> Option<&str> {
//         self.identity.as_deref()
//     }
// }

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
    #[arg(long, global(true), group = "environment-select")]
    ic: bool,
}

impl EnvironmentOpt {
    pub fn name(&self) -> &str {
        // Support --ic
        if self.ic {
            return "ic";
        }

        // Otherwise, default to `local`
        self.environment.as_deref().unwrap_or("local")
    }
}
