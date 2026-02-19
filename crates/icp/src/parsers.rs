//! Parsing of token and cycle amounts with support for suffixes (k, m, b, t) and underscores.

use bigdecimal::{BigDecimal, Signed};
use num_bigint::BigUint;
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};
use std::str::FromStr;

/// Parse a token amount with support for suffixes (k, m, b, t) and underscores.
///
/// Examples:
/// - `1` -> 1
/// - `1_000` -> 1000
/// - `1k` or `1K` -> 1000
/// - `1t` or `1T` -> 1000000000000
/// - `0.5` -> 0.5
/// - `0.5k` -> 500
pub fn parse_token_amount(input: &str) -> Result<BigDecimal, String> {
    let input = input.trim();

    if input.is_empty() {
        return Err("Token amount cannot be empty".to_string());
    }

    let (number_part, multiplier) = if let Some(last_char) = input.chars().last() {
        match last_char {
            'k' | 'K' => (&input[..input.len() - 1], 1_000u128),
            'm' | 'M' => (&input[..input.len() - 1], 1_000_000u128),
            'b' | 'B' => (&input[..input.len() - 1], 1_000_000_000u128),
            't' | 'T' => (&input[..input.len() - 1], 1_000_000_000_000u128),
            _ => (input, 1u128),
        }
    } else {
        (input, 1u128)
    };

    let cleaned = number_part.replace('_', "");
    let base =
        BigDecimal::from_str(&cleaned).map_err(|_| format!("Invalid token amount: '{}'", input))?;

    if base.is_negative() {
        return Err(format!("Token amount cannot be negative: '{}'", input));
    }

    let multiplier_decimal = BigDecimal::from(multiplier);
    Ok(base * multiplier_decimal)
}

/// Convert a token amount to the smallest unit by multiplying by 10^token_decimals.
/// E.g. 1.5 with 8 decimals -> 150000000. Fails if the result would be fractional.
pub fn to_token_unit_amount(
    token_amount: BigDecimal,
    token_decimals: u8,
) -> Result<BigUint, String> {
    use num_bigint::BigInt;
    use num_traits::pow::Pow;

    let (mantissa, exponent) = token_amount.into_bigint_and_exponent();
    let scale_adjustment = token_decimals as i64 - exponent;
    let ten = BigInt::from(10);

    let result = if scale_adjustment >= 0 {
        let multiplier = ten.pow(scale_adjustment as u32);
        mantissa * multiplier
    } else {
        let divisor = ten.pow((-scale_adjustment) as u32);
        let (quotient, remainder) = mantissa.div_rem(&divisor);
        if !remainder.is_zero() {
            return Err(format!(
                "Token amount cannot be represented with {} decimals (would result in fractional units)",
                token_decimals
            ));
        }
        quotient
    };

    result
        .try_into()
        .map_err(|_| "Token amount cannot be negative".to_string())
}

/// Parse a cycles amount with support for suffixes (k, m, b, t) and underscores.
/// Cycles have no decimal places, so the amount must be an integer.
///
/// Examples:
/// - `1` -> 1
/// - `1_000` -> 1000
/// - `1t` or `1T` -> 1000000000000
/// - `0.5t` -> 500_000_000_000
pub fn parse_cycles_amount(input: &str) -> Result<u128, String> {
    let token_amount = parse_token_amount(input)?;
    let unit_amount = to_token_unit_amount(token_amount, 0)?;
    unit_amount
        .to_u128()
        .ok_or_else(|| format!("Cycles amount too large: '{}'", input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cycles_plain() {
        assert_eq!(parse_cycles_amount("1").unwrap(), 1);
        assert_eq!(parse_cycles_amount("1000").unwrap(), 1000);
    }

    #[test]
    fn parse_cycles_suffixes() {
        assert_eq!(parse_cycles_amount("1k").unwrap(), 1000);
        assert_eq!(parse_cycles_amount("1t").unwrap(), 1_000_000_000_000);
        assert_eq!(parse_cycles_amount("4t").unwrap(), 4_000_000_000_000);
        assert_eq!(parse_cycles_amount("0.5t").unwrap(), 500_000_000_000);
    }

    #[test]
    fn parse_cycles_underscores() {
        assert_eq!(parse_cycles_amount("1_000").unwrap(), 1000);
    }

    #[test]
    fn parse_cycles_fractional_rejected() {
        assert!(parse_cycles_amount("1.5").is_err());
    }

    #[test]
    fn parse_token_amount_plain_and_suffixes() {
        use std::str::FromStr;
        assert_eq!(
            parse_token_amount("1").unwrap(),
            BigDecimal::from_str("1").unwrap()
        );
        assert_eq!(
            parse_token_amount("1k").unwrap(),
            BigDecimal::from_str("1000").unwrap()
        );
        assert_eq!(
            parse_token_amount("0.5t").unwrap(),
            BigDecimal::from_str("500000000000").unwrap()
        );
    }
}
