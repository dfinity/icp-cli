use clap::Subcommand;

pub(crate) mod balance;
pub(crate) mod buy;
pub(crate) mod mint;
pub(crate) mod transfer;

/// Mint and manage cycles
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Balance(balance::BalanceArgs),
    Buy(buy::BuyArgs),
    Mint(mint::MintArgs),
    Transfer(transfer::TransferArgs),
}
