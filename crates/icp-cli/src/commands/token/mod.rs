use clap::{Parser, Subcommand};

pub(crate) mod balance;
pub(crate) mod transfer;

#[derive(Debug, Parser)]
pub(crate) struct Command {
    /// The token to execute the operation on, defaults to `icp`
    #[arg(default_value = "icp")]
    pub(crate) token: String,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Balance(balance::BalanceArgs),
    Transfer(transfer::TransferArgs),
}
