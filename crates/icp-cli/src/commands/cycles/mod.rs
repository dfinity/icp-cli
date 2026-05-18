use clap::Subcommand;

pub(crate) mod balance;
pub(crate) mod mint;
pub(crate) mod transfer;

/// Mint and manage cycles
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Balance(balance::BalanceArgs),
    Mint(mint::MintArgs),
    Transfer(transfer::TransferArgs),
}
