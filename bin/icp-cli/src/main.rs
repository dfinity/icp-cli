use clap::Parser;
use commands::{Cmd, DispatchError};
use env::Env;
use icp_dirs::DiscoverDirsError;
use options::Format;
use snafu::{Snafu, report};

mod commands;
mod env;
mod options;

#[derive(Parser)]
struct Cli {
    #[arg(long, global = true)]
    identity: Option<String>,

    #[arg(long, global = true, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    #[command(flatten)]
    command: Cmd,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), ProgramError> {
    let cli = Cli::parse();
    let env = Env::builder()
        .identity(cli.identity)
        .output_format(cli.format)
        .build();
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
