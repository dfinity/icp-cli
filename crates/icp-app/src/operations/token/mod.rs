use bigdecimal::BigDecimal;
use candid::Nat;
use icp_canister_interfaces::icp_ledger::ICP_LEDGER_CID;
use num_bigint::ToBigInt;
use phf::phf_map;
use std::fmt;

pub mod allowance;
pub mod approve;
pub mod balance;
pub mod mint;
pub mod transfer;

/// A compile-time map of token names to their corresponding ledger canister ID and optional info overrides.
///
/// This map provides a quick lookup for well-known tokens on the Internet Computer:
/// - "icp": The Internet Computer Protocol token ledger canister
pub(super) static TOKEN_LEDGER_CIDS: phf::Map<&'static str, &'static str> = phf_map! {
    "icp" => ICP_LEDGER_CID,
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

/// Cycles for a human: `7.238T cycles`, `512.5B cycles`, `42 cycles`.
///
/// Trillions and billions are shown to at most three decimals with trailing
/// zeros dropped; anything smaller is the exact count. Output meant to be
/// parsed should carry the exact count instead.
pub fn format_cycles(cycles: u128) -> String {
    const TRILLION: u128 = 1_000_000_000_000;
    const BILLION: u128 = 1_000_000_000;
    match cycles {
        c if c >= TRILLION => format!("{}T cycles", scaled(c, TRILLION)),
        c if c >= BILLION => format!("{}B cycles", scaled(c, BILLION)),
        c => format!("{} cycles", Nat::from(c)),
    }
}

/// `cycles / unit` to three decimals, trailing zeros and a bare point dropped.
fn scaled(cycles: u128, unit: u128) -> String {
    // Split before scaling so the remainder, not the whole, is multiplied.
    let s = format!("{}.{:03}", cycles / unit, cycles % unit * 1000 / unit);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_cycles_trillions_and_billions() {
        assert_eq!(format_cycles(7_238_000_000_000), "7.238T cycles");
        assert_eq!(format_cycles(7_000_000_000_000), "7T cycles");
        assert_eq!(format_cycles(7_200_000_000_000), "7.2T cycles");
        assert_eq!(format_cycles(7_238_499_999_999), "7.238T cycles");
        assert_eq!(format_cycles(512_500_000_000), "512.5B cycles");
        assert_eq!(format_cycles(1_000_000_000), "1B cycles");
        assert_eq!(format_cycles(999_999_999), "999_999_999 cycles");
        assert_eq!(format_cycles(42), "42 cycles");
        assert_eq!(format_cycles(0), "0 cycles");
        // A gateway-supplied figure may be anything; no size may panic.
        assert!(format_cycles(u128::MAX).ends_with("T cycles"));
    }
}
