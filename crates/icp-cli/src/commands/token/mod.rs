use clap::{Parser, Subcommand};

pub(crate) mod balance;
pub(crate) mod transfer;

#[derive(Debug, Parser)]
pub(crate) struct Command {
    /// The token or ledger canister id to execute the operation on, defaults to `icp`
    #[arg(default_value = "icp", value_name = "TOKEN|LEDGER_ID")]
    pub(crate) token_name_or_ledger_id: String,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Display the token balance on the ledger (default token: icp)
    Balance(balance::BalanceArgs),

    /// Transfer ICP or ICRC1 tokens through their ledger (default token: icp)
    Transfer(transfer::TransferArgs),
}
