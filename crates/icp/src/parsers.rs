//! Parsing of token, cycle, memory, and duration amounts.
//!
//! Defined in `icp_deploy_canister::parsers` and re-exported here. What stays
//! is the parsing of inputs whose types the library cannot name.

use std::{fmt, str::FromStr};

pub use icp_deploy_canister::parsers::*;

/// A ledger account in either of the two textual shapes users supply: a
/// 32-byte hex ICP-ledger account identifier, or an ICRC-1 account.
#[derive(Debug, Clone, Copy)]
pub enum FlexibleAccountId {
    Icrc1(icrc_ledger_types::icrc1::account::Account),
    IcpLedger(ic_ledger_types::AccountIdentifier),
}

impl FromStr for FlexibleAccountId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try parsing as ICP ledger account (hex string)
        if let Ok(bytes) = hex::decode(s) {
            if bytes.len() == 32 {
                let mut array = [0u8; 32];
                array.copy_from_slice(&bytes);
                return Ok(FlexibleAccountId::IcpLedger(
                    ic_ledger_types::AccountIdentifier::from_slice(&array).unwrap(),
                ));
            } else {
                return Err(format!("Invalid ICP ledger account hex string: {s}"));
            }
        }
        // Try parsing as ICRC1 account
        if let Ok(account) = s.parse::<icrc_ledger_types::icrc1::account::Account>() {
            return Ok(FlexibleAccountId::Icrc1(account));
        }

        Err(format!("Invalid principal / account identifier: {s}"))
    }
}

impl fmt::Display for FlexibleAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlexibleAccountId::Icrc1(account) => account.fmt(f),
            FlexibleAccountId::IcpLedger(bytes) => hex::encode(bytes).fmt(f),
        }
    }
}
