use clap::{Parser, Subcommand};

pub(crate) mod balance;
pub(crate) mod transfer;

#[derive(Debug, Parser)]
pub(crate) struct Command {
    /// The token or principal to execute the operation on, defaults to `icp`
    #[arg(default_value = "icp")]
    pub(crate) token_name_or_principal: String,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Balance(balance::BalanceArgs),
    Transfer(transfer::TransferArgs),
}
