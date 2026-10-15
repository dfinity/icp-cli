//! A gateway's pricing, applied so the CLI can say what an amount buys
//! before any order exists, and find the least amount that buys a number of
//! cycles.
//!
//! The backend has no quote method. What it publishes is its XDR/USD rate
//! (one XDR is one trillion cycles) and, in its card terms, the arithmetic:
//! a card payment funds the amount less a fee of so many basis points plus a
//! fixed sum, rounded to the cent, and the net funds `net / rate` trillion
//! cycles. The figure is an estimate in two ways. The backend sets
//! `net_usd_cents` itself when it makes the order, and the cycles are minted
//! at the rate in force when the payment lands.

/// One XDR, in cycles.
pub const CYCLES_PER_XDR: u128 = 1_000_000_000_000;

/// The arithmetic a gateway publishes in its card terms, as the CLI applies
/// it: nothing here is the CLI's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Terms {
    /// The least a signed-in order may be, in US cents.
    pub min_order_usd_cents: u64,
    /// The most a signed-in order may be, in US cents.
    pub max_order_usd_cents: u64,
    /// The card fee's share of the amount, in basis points.
    pub fee_bps: u64,
    /// The card fee's fixed part, in US cents.
    pub fee_fixed_usd_cents: u64,
    /// What a Cycles Ledger delivery costs on top of a canister top-up, in
    /// cycles.
    pub ledger_delivery_fee_cycles: u128,
}

/// What a card payment of `gross_usd_cents` funds, in US cents, exactly as
/// the backend works it out: in floating point, with the fee as
/// `amount * fee_bps / 10_000 + fee_fixed_usd_cents`, rounded to cents as
/// JavaScript's `toFixed(2)` rounds, which the backend inherited from the
/// service it replaced. That is exact decimal rounding except where the
/// fee's share of the amount ends in half a cent (at CyclePay's 2.9%: $5,
/// $15, $25 and so on), where the double's exact value decides: $25 funds
/// $23.98 and $5 funds $4.55. A payment the fee exceeds funds nothing.
pub fn net_usd_cents(gross_usd_cents: u64, terms: &Terms) -> u64 {
    let gross_usd = gross_usd_cents as f64 / 100.0;
    // The backend divides the published integers the same way, so the
    // doubles here are the ones it uses: 290 / 10_000 is the double 0.029.
    let fee =
        gross_usd * (terms.fee_bps as f64 / 10_000.0) + terms.fee_fixed_usd_cents as f64 / 100.0;
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
pub fn card_fee_cents(gross_usd_cents: u64, terms: &Terms) -> u64 {
    gross_usd_cents - net_usd_cents(gross_usd_cents, terms)
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
pub fn ledger_cycles(gross_usd_cents: u64, xdr_usd_x10000: u64, terms: &Terms) -> u128 {
    cycles_for_net(net_usd_cents(gross_usd_cents, terms), xdr_usd_x10000)
        .saturating_sub(terms.ledger_delivery_fee_cycles)
}

/// The least amount within the terms' range whose [`ledger_cycles`] is at
/// least `cycles`, or `None` when the maximum itself does not buy them. The
/// minimum when it already does: a buyer cannot pay less than that, and gets
/// the more. Binary search, as the figure never falls when the amount rises.
pub fn least_gross_for(cycles: u128, xdr_usd_x10000: u64, terms: &Terms) -> Option<u64> {
    let (min, max) = (terms.min_order_usd_cents, terms.max_order_usd_cents);
    if min > max || ledger_cycles(max, xdr_usd_x10000, terms) < cycles {
        return None;
    }
    let (mut low, mut high) = (min, max);
    while low < high {
        let mid = low + (high - low) / 2;
        if ledger_cycles(mid, xdr_usd_x10000, terms) >= cycles {
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

    /// CyclePay's published terms: $1 to $200, Stripe's 2.9% + $0.30, and
    /// two Cycles Ledger fees of 100M cycles on a delivery.
    const CYCLEPAY: Terms = Terms {
        min_order_usd_cents: 100,
        max_order_usd_cents: 20_000,
        fee_bps: 290,
        fee_fixed_usd_cents: 30,
        ledger_delivery_fee_cycles: 200_000_000,
    };

    #[test]
    fn net_matches_the_backends_published_figures() {
        // CyclePay's README: $25 funds $23.98 and $5 funds $4.55, the two
        // half-cent ties going opposite ways.
        assert_eq!(net_usd_cents(2500, &CYCLEPAY), 2398);
        assert_eq!(net_usd_cents(500, &CYCLEPAY), 455);
        // Plain cases: 10 - 0.59, 1 - 0.329, 200 - 6.10.
        assert_eq!(net_usd_cents(1000, &CYCLEPAY), 941);
        assert_eq!(net_usd_cents(100, &CYCLEPAY), 67);
        assert_eq!(net_usd_cents(20_000, &CYCLEPAY), 19_390);
        // The fee exceeds the payment: nothing is funded, nothing underflows.
        assert_eq!(net_usd_cents(30, &CYCLEPAY), 0);
        assert_eq!(net_usd_cents(0, &CYCLEPAY), 0);
        assert_eq!(card_fee_cents(1000, &CYCLEPAY), 59);
    }

    /// The fee is whatever the gateway publishes: none at all, or another
    /// processor's, is applied as it is.
    #[test]
    fn the_fee_is_the_gateways() {
        let free = Terms {
            fee_bps: 0,
            fee_fixed_usd_cents: 0,
            ..CYCLEPAY
        };
        assert_eq!(net_usd_cents(2500, &free), 2500);
        assert_eq!(card_fee_cents(2500, &free), 0);
        // 1.5% + $0.25: $10 funds 10 - 0.15 - 0.25.
        let other = Terms {
            fee_bps: 150,
            fee_fixed_usd_cents: 25,
            ..CYCLEPAY
        };
        assert_eq!(net_usd_cents(1000, &other), 960);
        // A ledger fee the gateway sets is what comes off a delivery.
        let dear = Terms {
            ledger_delivery_fee_cycles: 1_000_000_000,
            ..CYCLEPAY
        };
        assert_eq!(
            ledger_cycles(2500, RATE, &dear),
            cycles_for_net(2398, RATE) - 1_000_000_000
        );
    }

    #[test]
    fn net_never_falls_as_the_amount_rises() {
        let mut previous = 0;
        for cents in 1..=25_000 {
            let net = net_usd_cents(cents, &CYCLEPAY);
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
            ledger_cycles(2500, RATE, &CYCLEPAY),
            17_736_686_390_532 - CYCLEPAY.ledger_delivery_fee_cycles
        );
        // A payment that funds nothing lands nothing, and does not underflow.
        assert_eq!(ledger_cycles(30, RATE, &CYCLEPAY), 0);
    }

    /// `cents` is the least amount that lands `cycles`: it does, and one cent
    /// less does not.
    fn assert_cheapest(cycles: u128, cents: u64) {
        assert!(
            ledger_cycles(cents, RATE, &CYCLEPAY) >= cycles,
            "{cents} c does not buy {cycles}"
        );
        assert!(
            ledger_cycles(cents - 1, RATE, &CYCLEPAY) < cycles,
            "{} c already buys {cycles}",
            cents - 1
        );
    }

    #[test]
    fn least_gross_is_the_cheapest_cent() {
        for target in [
            1_000_000_000_000u128,
            5_000_000_000_000,
            17_736_686_390_532 - CYCLEPAY.ledger_delivery_fee_cycles,
            100_000_000_000_000,
        ] {
            let cents = least_gross_for(target, RATE, &CYCLEPAY).unwrap();
            assert_cheapest(target, cents);
        }
        // The figure the index quotes for $25 is bought by exactly $25.
        assert_eq!(
            least_gross_for(
                17_736_686_390_532 - CYCLEPAY.ledger_delivery_fee_cycles,
                RATE,
                &CYCLEPAY
            ),
            Some(2500)
        );
    }

    #[test]
    fn least_gross_respects_the_range() {
        // Fewer cycles than the minimum buys: the minimum, not less.
        assert_eq!(least_gross_for(1, RATE, &CYCLEPAY), Some(100));
        assert_eq!(least_gross_for(0, RATE, &CYCLEPAY), Some(100));
        // More than the maximum buys: nothing.
        let most = ledger_cycles(20_000, RATE, &CYCLEPAY);
        assert_eq!(least_gross_for(most, RATE, &CYCLEPAY), Some(20_000));
        assert_eq!(least_gross_for(most + 1, RATE, &CYCLEPAY), None);
        let inverted = Terms {
            min_order_usd_cents: 200,
            max_order_usd_cents: 100,
            ..CYCLEPAY
        };
        assert_eq!(least_gross_for(1, RATE, &inverted), None);
        // No rate: nothing is bought at any amount.
        assert_eq!(least_gross_for(1, 0, &CYCLEPAY), None);
    }
}
