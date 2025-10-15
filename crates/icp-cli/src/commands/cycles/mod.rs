use clap::Subcommand;

use crate::commands::token;

pub(crate) mod balance;
pub(crate) mod mint;

#[derive(Subcommand, Debug)]
pub enum Command {
    Balance(token::balance::Cmd),
    Mint(mint::Cmd),
}
