use crate::context::Context;
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod balance;

#[derive(Debug, Parser)]
pub struct TokenArgs {
    /// Token identifier (name or canister ID). Defaults to "icp" when omitted.
    #[arg(value_name = "TOKEN")]
    token: Option<String>,
}

impl TokenArgs {
    pub fn token(&self) -> &str {
        self.token.as_deref().unwrap_or("icp")
    }
}

#[derive(Debug, Parser)]
#[command(subcommand_precedence_over_arg = true)]
pub struct Cmd {
    #[clap(flatten)]
    pub token_args: TokenArgs,

    #[command(subcommand)]
    subcmd: TokenSubcmd,
}

#[derive(Debug, Subcommand)]
pub enum TokenSubcmd {
    Balance(balance::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        TokenSubcmd::Balance(subcmd) => balance::exec(ctx, cmd.token_args, subcmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance { source: balance::CommandError },
}
