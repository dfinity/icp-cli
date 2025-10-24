use clap::{ArgGroup, Args};
use icp::identity::IdentitySelection;

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
        group = "environment-select"
    )]
    environment: Option<String>,

    /// Shorthand for --environment=ic.
    #[arg(long, global(true), group = "environment-select")]
    ic: bool,
}

impl EnvironmentOpt {
    pub(crate) fn name(&self) -> &str {
        // Support --ic
        if self.ic {
            return "ic";
        }

        // Otherwise, default to `local`
        self.environment.as_deref().unwrap_or("local")
    }

    pub(crate) fn is_explicit(&self) -> bool {
        self.environment.is_some() || self.ic
    }

    #[cfg(test)]
    pub(crate) fn with_environment(environment: impl Into<String>) -> Self {
        Self {
            environment: Some(environment.into()),
            ic: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_ic() -> Self {
        Self {
            environment: None,
            ic: true,
        }
    }
}
