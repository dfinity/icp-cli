use clap::{ArgGroup, Args};

#[derive(Args, Clone, Debug, Default)]
#[clap(
    group(ArgGroup::new("network-select").multiple(false)),
)]
pub struct NetworkOpt {
    /// Override the compute network to connect to. By default, the local network is used.
    #[arg(long, env = "ICP_NETWORK", global(true), group = "network-select")]
    network: Option<String>,

    /// Shorthand for --network=ic.
    #[clap(long, global(true), group = "network-select")]
    ic: bool,
}

impl NetworkOpt {
    pub fn name(&self) -> &str {
        if self.ic {
            "ic"
        } else {
            self.network.as_deref().unwrap_or("local")
        }
    }
}
