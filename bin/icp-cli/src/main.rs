use clap::Parser;
use commands::{Cmd, DispatchError};
use env::Env;
use icp_dirs::{DiscoverDirsError, IcpCliDirs};
use snafu::{Snafu, report};

mod commands;
mod env;
mod error;

#[derive(Parser)]
struct Cli {
    #[arg(long, global = true)]
    identity: Option<String>,
    #[command(flatten)]
    command: Cmd,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), ProgramError> {
    let cli = Cli::parse();
    let dirs = IcpCliDirs::new()?;
    let env = Env::new(dirs, cli.identity);
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
