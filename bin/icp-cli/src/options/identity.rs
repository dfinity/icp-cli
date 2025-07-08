use clap::Args;

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
