use bigdecimal::{BigDecimal, Signed, ToPrimitive};
use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_traits::{Zero, pow::Pow};
use std::str::FromStr;

/// Parse a token amount with support for suffixes (k, m, b, t) and underscores.
///
/// Examples:
/// - `1` -> 1
/// - `1000` -> 1000
/// - `1_000` -> 1000
/// - `1k` or `1K` -> 1000
/// - `1m` or `1M` -> 1000000
/// - `1b` or `1B` -> 1000000000
/// - `1t` or `1T` -> 1000000000000
/// - `0.5` -> 0.5
/// - `0.5k` or `0.5K` -> 500
pub(crate) fn parse_token_amount(input: &str) -> Result<BigDecimal, String> {
    let input = input.trim();

    if input.is_empty() {
        return Err("Token amount cannot be empty".to_string());
    }

    // Check if the last character is a suffix
    let (number_part, multiplier) = if let Some(last_char) = input.chars().last() {
        match last_char.to_ascii_lowercase() {
            'k' => (&input[..input.len() - 1], 1_000u128),
            'm' => (&input[..input.len() - 1], 1_000_000u128),
            'b' => (&input[..input.len() - 1], 1_000_000_000u128),
            't' => (&input[..input.len() - 1], 1_000_000_000_000u128),
            _ => (input, 1u128),
        }
    } else {
        (input, 1u128)
    };

    // Remove underscores from the number part
    let cleaned = number_part.replace('_', "");

    // Parse as BigDecimal to maintain precision
    let base =
        BigDecimal::from_str(&cleaned).map_err(|_| format!("Invalid token amount: '{}'", input))?;

    // Check for negative values
    if base.is_negative() {
        return Err(format!("Token amount cannot be negative: '{}'", input));
    }

    // Multiply by the multiplier
    let multiplier_decimal = BigDecimal::from(multiplier);
    let result = base * multiplier_decimal;

    Ok(result)
}

/// Convert a token amount (in token units) to the smallest unit amount by multiplying
/// by 10^token_decimals and checking that the result is an integer.
/// E.g. 1.5 ICP with 8 decimals = 150000000 e8s
///
/// # Arguments
/// * `token_amount` - The token amount in token units (e.g., 1.5 ICP)
/// * `token_decimals` - The number of decimals for the token (e.g., 8 for ICP)
///
/// # Returns
/// A `BigUint` representing the amount in the smallest unit (e.g., e8s for ICP)
///
/// # Errors
/// Returns an error if the result is not an integer after multiplication.
pub(crate) fn to_token_unit_amount(
    token_amount: BigDecimal,
    token_decimals: u8,
) -> Result<BigUint, String> {
    // Convert to internal representation: (mantissa, exponent)
    // where value = mantissa * 10^(-exponent)
    let (mantissa, exponent) = token_amount.into_bigint_and_exponent();

    // To convert to unit amount, we need to multiply by 10^token_decimals
    // mantissa * 10^(-exponent) * 10^token_decimals = mantissa * 10^(token_decimals - exponent)
    let scale_adjustment = token_decimals as i64 - exponent;

    let ten = BigInt::from(10);
    let result = if scale_adjustment >= 0 {
        // Multiply by 10^scale_adjustment
        let multiplier = ten.pow(scale_adjustment as u32);
        mantissa * multiplier
    } else {
        // Divide by 10^(-scale_adjustment), checking for remainder
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

    // Convert to BigUint (should always be non-negative since we validated earlier)
    result
        .try_into()
        .map_err(|_| "Token amount cannot be negative".to_string())
}

/// Parse a cycles amount with support for suffixes (k, m, b, t) and underscores.
/// Cycles have no decimal places, so the amount must be an integer.
///
/// Examples:
/// - `1` -> 1
/// - `1000` -> 1000
/// - `1_000` -> 1000
/// - `1k` or `1K` -> 1000
/// - `1m` or `1M` -> 1000000
/// - `1b` or `1B` -> 1000000000
/// - `1t` or `1T` -> 1000000000000
/// - `0.5k` or `0.5K` -> 500
pub(crate) fn parse_cycles_amount(input: &str) -> Result<u128, String> {
    let token_amount = parse_token_amount(input)?;
    let unit_amount = to_token_unit_amount(token_amount, 0)?;
    unit_amount
        .to_u128()
        .ok_or_else(|| format!("Cycles amount too large: '{}'", input))
}

pub(crate) fn parse_root_key(input: &str) -> Result<Vec<u8>, String> {
    hex::decode(input).map_err(|e| format!("Invalid root key hex string: {e}"))
}

pub(crate) fn parse_subaccount(input: &str) -> Result<[u8; 32], String> {
    if input.len() > 64 {
        return Err(format!(
            "Subaccount cannot be longer than 64 hex characters: '{}'",
            input
        ));
    }
    let padded = format!("{:0>64}", input);
    let bytes =
        hex::decode(padded).map_err(|_| format!("Invalid subaccount hex string: '{input}'",))?;
    Ok(bytes
        .try_into()
        .expect("Hex string should be 32 bytes after padding"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for parse_token_amount
    #[test]
    fn test_parse_token_amount_plain_numbers() {
        assert_eq!(
            parse_token_amount("1").unwrap(),
            BigDecimal::from_str("1").unwrap()
        );
        assert_eq!(
            parse_token_amount("1000").unwrap(),
            BigDecimal::from_str("1000").unwrap()
        );
        assert_eq!(
            parse_token_amount("123456789").unwrap(),
            BigDecimal::from_str("123456789").unwrap()
        );
    }

    #[test]
    fn test_parse_token_amount_with_decimals() {
        assert_eq!(
            parse_token_amount("0.5").unwrap(),
            BigDecimal::from_str("0.5").unwrap()
        );
        assert_eq!(
            parse_token_amount("1.25").unwrap(),
            BigDecimal::from_str("1.25").unwrap()
        );
        assert_eq!(
            parse_token_amount("123.456789").unwrap(),
            BigDecimal::from_str("123.456789").unwrap()
        );
    }

    #[test]
    fn test_parse_token_amount_with_underscores() {
        assert_eq!(
            parse_token_amount("1_000").unwrap(),
            BigDecimal::from_str("1000").unwrap()
        );
        assert_eq!(
            parse_token_amount("1_000_000").unwrap(),
            BigDecimal::from_str("1000000").unwrap()
        );
    }

    #[test]
    fn test_parse_token_amount_with_suffixes() {
        assert_eq!(
            parse_token_amount("1k").unwrap(),
            BigDecimal::from_str("1000").unwrap()
        );
        assert_eq!(
            parse_token_amount("1.5m").unwrap(),
            BigDecimal::from_str("1500000").unwrap()
        );
        assert_eq!(
            parse_token_amount("2b").unwrap(),
            BigDecimal::from_str("2000000000").unwrap()
        );
        assert_eq!(
            parse_token_amount("0.5t").unwrap(),
            BigDecimal::from_str("500000000000").unwrap()
        );
        assert_eq!(
            parse_token_amount("1K").unwrap(),
            BigDecimal::from_str("1000").unwrap()
        );
        assert_eq!(
            parse_token_amount("1.5M").unwrap(),
            BigDecimal::from_str("1500000").unwrap()
        );
        assert_eq!(
            parse_token_amount("2B").unwrap(),
            BigDecimal::from_str("2000000000").unwrap()
        );
        assert_eq!(
            parse_token_amount("0.5T").unwrap(),
            BigDecimal::from_str("500000000000").unwrap()
        );
    }

    #[test]
    fn test_parse_token_amount_errors() {
        assert!(parse_token_amount("").is_err());
        assert!(parse_token_amount("abc").is_err());
        assert!(parse_token_amount("1.2.3").is_err());
        assert!(parse_token_amount("-1").is_err());
    }

    // Tests for to_token_unit_amount
    #[test]
    fn test_to_token_unit_amount_integer_result() {
        // 1 ICP with 8 decimals = 100000000 e8s
        let amount = BigDecimal::from_str("1").unwrap();
        let result = to_token_unit_amount(amount, 8).unwrap();
        assert_eq!(result, BigUint::from(100_000_000u128));

        // 0.5 ICP with 8 decimals = 50000000 e8s
        let amount = BigDecimal::from_str("0.5").unwrap();
        let result = to_token_unit_amount(amount, 8).unwrap();
        assert_eq!(result, BigUint::from(50_000_000u128));

        // 1.12345678 ICP with 8 decimals = 112345678 e8s
        let amount = BigDecimal::from_str("1.12345678").unwrap();
        let result = to_token_unit_amount(amount, 8).unwrap();
        assert_eq!(result, BigUint::from(112_345_678u128));
    }

    #[test]
    fn test_to_token_unit_amount_zero_decimals() {
        // Cycles have 0 decimals
        let amount = BigDecimal::from_str("1000").unwrap();
        let result = to_token_unit_amount(amount, 0).unwrap();
        assert_eq!(result, BigUint::from(1000u128));
    }

    #[test]
    fn test_to_token_unit_amount_fractional_error() {
        // 1.123456789 ICP with 8 decimals would result in a fractional unit
        let amount = BigDecimal::from_str("1.123456789").unwrap();
        let result = to_token_unit_amount(amount, 8);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be represented"));
    }

    #[test]
    fn test_to_token_unit_amount_large_numbers() {
        // Very large amount
        let amount = BigDecimal::from_str("1000000000000").unwrap();
        let result = to_token_unit_amount(amount, 8).unwrap();
        assert_eq!(result, BigUint::from(100_000_000_000_000_000_000u128));
    }

    // Tests for parse_cycles_amount
    #[test]
    fn test_parse_cycles_plain_numbers() {
        assert_eq!(parse_cycles_amount("1").unwrap(), 1);
        assert_eq!(parse_cycles_amount("1000").unwrap(), 1000);
        assert_eq!(parse_cycles_amount("123456789").unwrap(), 123456789);
    }

    #[test]
    fn test_parse_cycles_with_underscores() {
        assert_eq!(parse_cycles_amount("1_000").unwrap(), 1000);
        assert_eq!(parse_cycles_amount("1_000_000").unwrap(), 1000000);
        assert_eq!(parse_cycles_amount("123_456_789").unwrap(), 123456789);
    }

    #[test]
    fn test_parse_cycles_with_k_suffix() {
        assert_eq!(parse_cycles_amount("1k").unwrap(), 1000);
        assert_eq!(parse_cycles_amount("1K").unwrap(), 1000);
        assert_eq!(parse_cycles_amount("5k").unwrap(), 5000);
        assert_eq!(parse_cycles_amount("1.5k").unwrap(), 1500);
    }

    #[test]
    fn test_parse_cycles_with_m_suffix() {
        assert_eq!(parse_cycles_amount("1m").unwrap(), 1000000);
        assert_eq!(parse_cycles_amount("1M").unwrap(), 1000000);
        assert_eq!(parse_cycles_amount("5m").unwrap(), 5000000);
        assert_eq!(parse_cycles_amount("2.5m").unwrap(), 2500000);
    }

    #[test]
    fn test_parse_cycles_with_b_suffix() {
        assert_eq!(parse_cycles_amount("1b").unwrap(), 1000000000);
        assert_eq!(parse_cycles_amount("1B").unwrap(), 1000000000);
        assert_eq!(parse_cycles_amount("3b").unwrap(), 3000000000);
        assert_eq!(parse_cycles_amount("1.5b").unwrap(), 1500000000);
    }

    #[test]
    fn test_parse_cycles_with_t_suffix() {
        assert_eq!(parse_cycles_amount("1t").unwrap(), 1000000000000);
        assert_eq!(parse_cycles_amount("1T").unwrap(), 1000000000000);
        assert_eq!(parse_cycles_amount("2t").unwrap(), 2000000000000);
        assert_eq!(parse_cycles_amount("0.5t").unwrap(), 500000000000);
    }

    #[test]
    fn test_parse_cycles_with_decimal_and_underscores() {
        assert_eq!(parse_cycles_amount("1_000k").unwrap(), 1000000);
        assert_eq!(parse_cycles_amount("1_000_000").unwrap(), 1000000);
    }

    #[test]
    fn test_parse_cycles_errors() {
        assert!(parse_cycles_amount("").is_err());
        assert!(parse_cycles_amount("abc").is_err());
        assert!(parse_cycles_amount("1.2.3").is_err());
        assert!(parse_cycles_amount("k").is_err());
    }

    #[test]
    fn test_parse_cycles_fractional_error() {
        // Cycles must be integers
        let err = parse_cycles_amount("1.5").unwrap_err();
        assert!(err.contains("cannot be represented"));
    }

    #[test]
    fn test_parse_cycles_large_numbers() {
        // Test very large numbers that would lose precision with f64
        assert_eq!(
            parse_cycles_amount("340282366920938463463374607431768211455").unwrap(),
            340282366920938463463374607431768211455u128
        ); // u128::MAX

        // Large number with suffix that fits in u128 (340 trillion trillion)
        assert_eq!(
            parse_cycles_amount("340t").unwrap(),
            340_000_000_000_000u128
        );

        // Another large number that would lose precision with f64 (18 digits)
        assert_eq!(
            parse_cycles_amount("123456789012345678901234567890").unwrap(),
            123456789012345678901234567890u128
        );

        // Very large with decimal and suffix
        assert_eq!(
            parse_cycles_amount("99999999999999999999t").unwrap(),
            99999999999999999999000000000000u128
        );

        // Decimal precision maintained (integer result)
        assert_eq!(parse_cycles_amount("1.999999t").unwrap(), 1999999000000);
    }

    #[test]
    fn test_parse_cycles_overflow() {
        // Should overflow u128::MAX (340282366920938463463374607431768211455)
        let err1 = parse_cycles_amount("340282366920938463463374607431768211456").unwrap_err();
        assert!(err1.contains("Cycles amount too large"));

        // Very large number that definitely overflows
        let err3 = parse_cycles_amount("999999999999999999999999999999t").unwrap_err();
        assert!(err3.contains("Cycles amount too large"));
    }
}
