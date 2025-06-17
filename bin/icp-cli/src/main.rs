use crate::canister_store::CanisterStore;
use camino::Utf8PathBuf;
use clap::Parser;
use commands::{Cmd, DispatchError};
use env::Env;
use icp_dirs::{DiscoverDirsError, IcpCliDirs};
use snafu::{Snafu, report};

mod canister_store;
mod commands;
mod env;

#[derive(Parser)]
struct Cli {
    #[arg(long, global = true)]
    identity: Option<String>,

    #[arg(long, default_value = "ids.json")]
    store: Utf8PathBuf,

    #[command(flatten)]
    command: Cmd,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), ProgramError> {
    let cli = Cli::parse();

    // Setup project directory structure
    let dirs = IcpCliDirs::new()?;

    // Canister Store
    let cs = CanisterStore::new(&cli.store);

    // Setup environment
    let env = Env::new(
        dirs,         // dirs
        cli.identity, // identity
        cs,           // canister_store
    );

    commands::dispatch(&env, cli.command).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ProgramError {
    #[snafu(transparent)]
    Dispatch { source: DispatchError },

    #[snafu(transparent)]
    Dirs { source: DiscoverDirsError },
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    #[test]
    fn valid_command() {
        Cli::command().debug_assert();
    }
}
