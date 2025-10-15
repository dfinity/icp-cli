use clap::Subcommand;
use icp_canister_interfaces::{cycles_ledger::CYCLES_LEDGER_CID, icp_ledger::ICP_LEDGER_CID};
use phf::phf_map;

pub(crate) mod balance;
pub(crate) mod transfer;

/// A compile-time map of token names to their corresponding ledger canister IDs.
///
/// This map provides a quick lookup for well-known tokens on the Internet Computer:
/// - "icp": The Internet Computer Protocol token ledger canister
/// - "cycles": The cycles ledger canister for managing computation cycles
///
/// The canister IDs are stored as string literals in textual format.
static TOKEN_LEDGER_CIDS: phf::Map<&'static str, &'static str> = phf_map! {
    "icp" => ICP_LEDGER_CID,
    "cycles" => CYCLES_LEDGER_CID,
};

#[derive(Subcommand, Debug)]
pub enum Command {
    Balance(balance::Cmd),
    Transfer(transfer::Cmd),
}
