use clap::Subcommand;

pub(crate) mod balance;
pub(crate) mod mint;
pub(crate) mod transfer;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Display the cycles balance
    Balance(balance::BalanceArgs),

    /// Convert icp to cycles
    Mint(mint::MintArgs),

    /// Transfer cycles to another principal
    Transfer(transfer::TransferArgs),
}
