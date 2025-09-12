use clap::{Parser, Subcommand};
use phf::phf_map;
use snafu::Snafu;

use crate::context::Context;

mod balance;
mod transfer;

/// A compile-time map of token names to their corresponding ledger canister IDs.
///
/// This map provides a quick lookup for well-known tokens on the Internet Computer:
/// - "icp": The Internet Computer Protocol token ledger canister
/// - "cycles": The cycles ledger canister for managing computation cycles
///
/// The canister IDs are stored as string literals in textual format.
static TOKEN_LEDGER_CIDS: phf::Map<&'static str, &'static str> = phf_map! {
    "icp" => "ryjl3-tyaaa-aaaaa-aaaba-cai",
    "cycles" => "um5iw-rqaaa-aaaaq-qaaba-cai",
};

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Balance(balance::Cmd),
    Transfer(transfer::Cmd),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        Subcmd::Balance(cmd) => balance::exec(ctx, cmd).await?,
        Subcmd::Transfer(cmd) => transfer::exec(ctx, cmd).await?,
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
