use clap::Subcommand;

use crate::commands::token;

pub(crate) mod balance;
pub(crate) mod mint;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Balance(token::balance::BalanceArgs),
    Mint(mint::MintArgs),
}
