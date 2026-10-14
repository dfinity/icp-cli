//! Buying cycles with a card through CyclePay.
//!
//! A vendor integration, kept to this crate: everything about CyclePay's
//! canisters and their order interface lives under this module, and nothing
//! outside the `cycles buy` command reaches into it. The CLI carries nothing
//! of CyclePay's but its backend's canister id: the Payment Link, the order
//! limits and the fees are read from the gateway, so any canister serving the
//! same interface can stand in for it.
//!
//! - [`interface`]: the backend canister's Candid, as far as the CLI uses it.
//! - [`pricing`]: the gateway's arithmetic, applied for quoting before an
//!   order exists.
//! - [`purchase`]: the operation — quote, create, read back, wait. It never
//!   prints; progress goes out as [`icp_events`] tasks.
//! - [`task`]: those tasks, one per leg of the wait, and how they are drawn.
//! - [`money`]: a sum in a fiat currency, in minor units.

pub(crate) mod interface;
pub(crate) mod money;
pub(crate) mod pricing;
pub(crate) mod purchase;
pub(crate) mod task;

#[cfg(test)]
mod tests;

use candid::Nat;

/// Cycles for a human: `7.238T cycles`, `512.5B cycles`, `42 cycles`.
///
/// Trillions and billions are shown to at most three decimals with trailing
/// zeros dropped; anything smaller is the exact count. Output meant to be
/// parsed should carry the exact count instead.
pub(crate) fn format_cycles(cycles: u128) -> String {
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
mod format_tests {
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
        // A backend-supplied figure may be anything; no size may panic.
        assert!(format_cycles(u128::MAX).ends_with("T cycles"));
    }
}
