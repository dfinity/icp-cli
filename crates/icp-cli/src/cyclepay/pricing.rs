//! CyclePay's pricing, reproduced so the CLI can say what an amount buys
//! before any order exists, and find the least amount that buys a number of
//! cycles.
//!
//! The backend has no quote method. What it publishes is its XDR/USD rate
//! (one XDR is one trillion cycles) and the arithmetic: a card payment funds
//! the amount less Stripe's 2.9% plus 30 cents, rounded to the cent, and the
//! net funds `net / rate` trillion cycles. The figure is an estimate in two
//! ways. The backend sets `net_usd_cents` itself when it makes the order,
//! and the cycles are minted at the rate in force when the payment lands.

/// What a Cycles Ledger delivery costs on top of a canister top-up: two
/// Cycles Ledger fees of 100M cycles.
pub const LEDGER_DELIVERY_FEE_CYCLES: u128 = 200_000_000;

/// One XDR, in cycles.
pub const CYCLES_PER_XDR: u128 = 1_000_000_000_000;

/// What a card payment of `gross_usd_cents` funds, in US cents, exactly as
/// the backend works it out: in floating point, rounded to cents as
/// JavaScript's `toFixed(2)` rounds, which the backend inherited from the
/// service it replaced. That is exact decimal rounding except where 2.9% of
/// the amount ends in half a cent ($5, $15, $25 and so on), where the
/// double's exact value decides: $25 funds $23.98 and $5 funds $4.55. A
/// payment the fee exceeds funds nothing.
pub fn net_usd_cents(gross_usd_cents: u64) -> u64 {
    let gross_usd = gross_usd_cents as f64 / 100.0;
    let fee = gross_usd * 0.029 + 0.3;
    js_to_fixed_2_in_cents(gross_usd - fee).max(0) as u64
}

/// `Math.round(+x.toFixed(2) * 100)`: `x` rounded to cents on its exact
/// binary value, an exact tie going away from zero.
fn js_to_fixed_2_in_cents(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    let bits = x.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, exp2) = if exponent == 0 {
        (fraction, -1074i64)
    } else {
        (fraction | (1u64 << 52), exponent - 1075)
    };
    // |x| * 100 = mantissa * 100 * 2^exp2, exactly.
    let scaled = (mantissa as u128) * 100;
    let cents = if exp2 >= 0 {
        u32::try_from(exp2)
            .ok()
            .and_then(|e| scaled.checked_shl(e))
            .unwrap_or(u128::MAX)
    } else if -exp2 >= 120 {
        0
    } else {
        let shift = (-exp2) as u32;
        (scaled + (1u128 << (shift - 1))) >> shift
    };
    let cents = i64::try_from(cents).unwrap_or(i64::MAX);
    if x < 0.0 { -cents } else { cents }
}

/// The card fee on `gross_usd_cents`: the payment less what it funds.
pub fn card_fee_cents(gross_usd_cents: u64) -> u64 {
    gross_usd_cents - net_usd_cents(gross_usd_cents)
}

/// The cycles `net_usd_cents` mints at `xdr_usd_x10000` US dollars per
/// 10,000 XDR, rounded down. Zero at a rate of zero, which no quote accepts.
pub fn cycles_for_net(net_usd_cents: u64, xdr_usd_x10000: u64) -> u128 {
    // net_usd_cents / 100 dollars, at xdr_usd_x10000 / 10_000 dollars an XDR,
    // is net_usd_cents * 100 / xdr_usd_x10000 XDR.
    (u128::from(net_usd_cents) * CYCLES_PER_XDR * 100)
        .checked_div(u128::from(xdr_usd_x10000))
        .unwrap_or(0)
}

/// The cycles a card payment of `gross_usd_cents` lands on a Cycles Ledger
/// account: what its net mints, less the delivery's ledger fees.
pub fn ledger_cycles(gross_usd_cents: u64, xdr_usd_x10000: u64) -> u128 {
    cycles_for_net(net_usd_cents(gross_usd_cents), xdr_usd_x10000)
        .saturating_sub(LEDGER_DELIVERY_FEE_CYCLES)
}

/// The least amount in `min..=max` cents whose [`ledger_cycles`] is at least
/// `cycles`, or `None` when `max` itself does not buy them. The minimum when
/// it already does: a buyer cannot pay less than that, and gets the more.
/// Binary search, as the figure never falls when the amount rises.
pub fn least_gross_for(cycles: u128, xdr_usd_x10000: u64, min: u64, max: u64) -> Option<u64> {
    if min > max || ledger_cycles(max, xdr_usd_x10000) < cycles {
        return None;
    }
    let (mut low, mut high) = (min, max);
    while low < high {
        let mid = low + (high - low) / 2;
        if ledger_cycles(mid, xdr_usd_x10000) >= cycles {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    Some(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plausible rate: $1.3520 an XDR.
    const RATE: u64 = 13_520;

    #[test]
    fn net_matches_the_backends_published_figures() {
        // CyclePay's README: $25 funds $23.98 and $5 funds $4.55, the two
        // half-cent ties going opposite ways.
        assert_eq!(net_usd_cents(2500), 2398);
        assert_eq!(net_usd_cents(500), 455);
        // Plain cases: 10 - 0.59, 1 - 0.329, 200 - 6.10.
        assert_eq!(net_usd_cents(1000), 941);
        assert_eq!(net_usd_cents(100), 67);
        assert_eq!(net_usd_cents(20_000), 19_390);
        // The fee exceeds the payment: nothing is funded, nothing underflows.
        assert_eq!(net_usd_cents(30), 0);
        assert_eq!(net_usd_cents(0), 0);
        assert_eq!(card_fee_cents(1000), 59);
    }

    #[test]
    fn net_never_falls_as_the_amount_rises() {
        let mut previous = 0;
        for cents in 1..=25_000 {
            let net = net_usd_cents(cents);
            assert!(net >= previous, "{cents} c funds {net} after {previous}");
            previous = net;
        }
    }

    #[test]
    fn cycles_follow_the_rate() {
        // CyclePay's agent index: $23.98 at $1.352 an XDR is 17.73T, down.
        let cycles = cycles_for_net(2398, RATE);
        assert_eq!(cycles, 17_736_686_390_532);
        assert_eq!(cycles_for_net(0, RATE), 0);
        assert_eq!(cycles_for_net(2398, 0), 0);
        // Ten XDR exactly: $13.52 at $1.352 an XDR.
        assert_eq!(cycles_for_net(1352, RATE), 10 * CYCLES_PER_XDR);
        assert_eq!(
            ledger_cycles(2500, RATE),
            17_736_686_390_532 - LEDGER_DELIVERY_FEE_CYCLES
        );
        // A payment that funds nothing lands nothing, and does not underflow.
        assert_eq!(ledger_cycles(30, RATE), 0);
    }

    /// `cents` is the least amount that lands `cycles`: it does, and one cent
    /// less does not.
    fn assert_cheapest(cycles: u128, cents: u64) {
        assert!(
            ledger_cycles(cents, RATE) >= cycles,
            "{cents} c does not buy {cycles}"
        );
        assert!(
            ledger_cycles(cents - 1, RATE) < cycles,
            "{} c already buys {cycles}",
            cents - 1
        );
    }

    #[test]
    fn least_gross_is_the_cheapest_cent() {
        for target in [
            1_000_000_000_000u128,
            5_000_000_000_000,
            17_736_686_390_532 - LEDGER_DELIVERY_FEE_CYCLES,
            100_000_000_000_000,
        ] {
            let cents = least_gross_for(target, RATE, 100, 20_000).unwrap();
            assert_cheapest(target, cents);
        }
        // The figure the index quotes for $25 is bought by exactly $25.
        assert_eq!(
            least_gross_for(
                17_736_686_390_532 - LEDGER_DELIVERY_FEE_CYCLES,
                RATE,
                100,
                20_000
            ),
            Some(2500)
        );
    }

    #[test]
    fn least_gross_respects_the_range() {
        // Fewer cycles than the minimum buys: the minimum, not less.
        assert_eq!(least_gross_for(1, RATE, 100, 20_000), Some(100));
        assert_eq!(least_gross_for(0, RATE, 100, 20_000), Some(100));
        // More than the maximum buys: nothing.
        let most = ledger_cycles(20_000, RATE);
        assert_eq!(least_gross_for(most, RATE, 100, 20_000), Some(20_000));
        assert_eq!(least_gross_for(most + 1, RATE, 100, 20_000), None);
        assert_eq!(least_gross_for(1, RATE, 200, 100), None);
        // No rate: nothing is bought at any amount.
        assert_eq!(least_gross_for(1, 0, 100, 20_000), None);
    }
}
