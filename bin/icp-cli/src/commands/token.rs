use crate::context::Context;
use candid::Principal;
use clap::{Parser, Subcommand};
use snafu::Snafu;

pub mod balance;
pub mod transfer;

#[derive(Debug, Parser)]
pub struct TokenArgs {
    /// Token identifier (name or canister ID). Defaults to "icp" when omitted.
    #[arg(value_name = "TOKEN")]
    pub token: Option<String>,
}

impl TokenArgs {
    pub fn token(&self) -> &str {
        self.token.as_deref().unwrap_or("icp")
    }

    pub fn token_address(&self) -> Option<Principal> {
        if let Ok(token_address) = Principal::from_text(self.token()) {
            return Some(token_address);
        }

        match self.token().to_lowercase().as_str() {
            "icp" => Some(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap()),
            "cycles" => Some(Principal::from_text("um5iw-rqaaa-aaaaq-qaaba-cai").unwrap()),
            _ => None,
        }
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
    Transfer(transfer::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        TokenSubcmd::Balance(subcmd) => balance::exec(ctx, cmd.token_args, subcmd).await?,
        TokenSubcmd::Transfer(subcmd) => transfer::exec(ctx, cmd.token_args, subcmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance { source: balance::CommandError },

    #[snafu(transparent)]
    Transfer { source: transfer::CommandError },
}
