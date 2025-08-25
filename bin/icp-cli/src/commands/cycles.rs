use crate::{
    commands::token::{self, TokenArgs},
    context::Context,
};
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod mint;

fn cycles_token_args() -> TokenArgs {
    TokenArgs {
        token: Some(String::from("cycles")),
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
    Balance(token::balance::Cmd),
    Mint(mint::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        TokenSubcmd::Balance(subcmd) => {
            token::balance::exec(ctx, cycles_token_args(), subcmd).await?
        }
        TokenSubcmd::Mint(subcmd) => mint::exec(ctx, subcmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance {
        source: token::balance::CommandError,
    },

    #[snafu(transparent)]
    Mint { source: mint::CommandError },
}
