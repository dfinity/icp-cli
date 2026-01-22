use bigdecimal::BigDecimal;
use candid::Nat;
use icp_canister_interfaces::{cycles_ledger::CYCLES_LEDGER_CID, icp_ledger::ICP_LEDGER_CID};
use num_bigint::ToBigInt;
use phf::phf_map;
use std::fmt;

pub(crate) mod balance;
pub(crate) mod mint;
pub(crate) mod transfer;

#[derive(Debug, Clone)]
pub(super) struct TokenInfoOverride {
    decimals: u8,
    symbol: &'static str,
}

/// A compile-time map of token names to their corresponding ledger canister ID and optional info overrides.
///
/// This map provides a quick lookup for well-known tokens on the Internet Computer:
/// - "icp": The Internet Computer Protocol token ledger canister
/// - "cycles": The cycles ledger canister for managing computation cycles
///
/// If the override info is provided, icp-cli should not fetch the info from the token ledger canister.
pub(super) static TOKEN_LEDGER_INFO: phf::Map<
    &'static str,
    (&'static str, Option<TokenInfoOverride>),
> = phf_map! {
    "icp" => (ICP_LEDGER_CID, None),
    "cycles" => (CYCLES_LEDGER_CID, Some(TokenInfoOverride { decimals: 0, symbol: "cycles" }))
};

/// Represents a token amount with its symbol for display purposes.
#[derive(Debug)]
pub struct TokenAmount {
    pub amount: BigDecimal,
    pub symbol: String,
}

impl fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted_amount = if self.amount.fractional_digit_count() == 0 {
            // No decimals - format with underscores
            format_integer_with_underscores(&self.amount)
        } else {
            // Has decimals - display as is
            self.amount.to_string()
        };
        write!(f, "{} {}", formatted_amount, self.symbol)
    }
}

fn format_integer_with_underscores(amount: &BigDecimal) -> String {
    // Nat displays numbers with underscores
    if let Some(bigint) = amount.to_bigint()
        && let Some(biguint) = bigint.to_biguint()
    {
        return format!("{}", Nat::from(biguint));
    }
    // Fallback to plain string if conversion fails
    amount.to_string()
}
