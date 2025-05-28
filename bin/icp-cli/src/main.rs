use clap::{Parser, ValueEnum};
use commands::{Cmd, DispatchError};
use env::Env;
use snafu::report;

mod commands;
mod env;
mod error;
mod project;

#[derive(Parser)]
struct Cli {
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Human)]
    output_format: OutputFormat,
    #[arg(long, global = true)]
    identity: Option<String>,
    #[command(flatten)]
    command: Cmd,
}

#[derive(ValueEnum, Debug, Copy, Clone, Eq, PartialEq)]
enum OutputFormat {
    Human,
    Json,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), DispatchError> {
    let cli = Cli::parse();
    let env = Env::new(cli.output_format, cli.identity);
    commands::dispatch(&env, cli.command).await?;
    Ok(())
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
