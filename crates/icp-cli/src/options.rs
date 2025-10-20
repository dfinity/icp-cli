use clap::Args;
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
