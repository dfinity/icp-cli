//! Parsing of token and cycle amounts with support for suffixes (k, m, b, t) and underscores.

use bigdecimal::{BigDecimal, Signed};
use num_bigint::BigUint;
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
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

/// An amount of cycles.
///
/// Deserializes from a number or a string with suffixes (k, m, b, t) and optional underscore separators. Serializes as a number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CyclesAmount(pub u128);

impl<'de> Deserialize<'de> for CyclesAmount {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOf {
            // u128 is not supported by serde_yaml/serde_json; use u64 so numeric input deserializes.
            Number(u64),
            Str(String),
        }

        const HINT: &str = "cycles amount must be a number or a string with optional suffix (k, m, b, t), e.g. 1000 or \"4t\"";
        let v = OneOf::deserialize(d).map_err(|_| serde::de::Error::custom(HINT))?;
        let s = match &v {
            OneOf::Number(n) => n.to_string(),
            OneOf::Str(s) => s.clone(),
        };
        let c = s
            .parse::<CyclesAmount>()
            .map_err(serde::de::Error::custom)?;
        Ok(c)
    }
}

impl Serialize for CyclesAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as string for readability of large numbers in JSON; u128 is not in JSON spec
        serializer.serialize_str(&self.0.to_string())
    }
}

impl JsonSchema for CyclesAmount {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("CyclesAmount")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Cycles amount. Accepts a number or a string with suffixes (k, m, b, t), e.g. \"4t\" or \"4.3t\".",
            "oneOf": [
                { "type": "integer", "minimum": 0 },
                { "type": "string", "description": "Amount with optional suffix: k, m, b, t" }
            ]
        })
    }
}

impl FromStr for CyclesAmount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let token_amount = parse_token_amount(s)?;
        let unit_amount = to_token_unit_amount(token_amount, 0)?;
        let n = unit_amount
            .to_u128()
            .ok_or_else(|| format!("Cycles amount too large: '{}'", s))?;
        Ok(CyclesAmount(n))
    }
}

impl fmt::Display for CyclesAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<CyclesAmount> for u128 {
    fn from(c: CyclesAmount) -> Self {
        c.0
    }
}

impl From<u128> for CyclesAmount {
    fn from(n: u128) -> Self {
        CyclesAmount(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_amount_from_str_plain() {
        assert_eq!("1".parse::<CyclesAmount>().unwrap().0, 1);
        assert_eq!("1000".parse::<CyclesAmount>().unwrap().0, 1000);
    }

    #[test]
    fn cycles_amount_from_str_suffixes() {
        assert_eq!("1k".parse::<CyclesAmount>().unwrap().0, 1000);
        assert_eq!("1t".parse::<CyclesAmount>().unwrap().0, 1_000_000_000_000);
        assert_eq!("4t".parse::<CyclesAmount>().unwrap().0, 4_000_000_000_000);
        assert_eq!("0.5t".parse::<CyclesAmount>().unwrap().0, 500_000_000_000);
    }

    #[test]
    fn cycles_amount_from_str_underscores() {
        assert_eq!("1_000".parse::<CyclesAmount>().unwrap().0, 1000);
    }

    #[test]
    fn cycles_amount_from_str_fractional_rejected() {
        assert!("1.5".parse::<CyclesAmount>().is_err());
    }

    #[test]
    fn cycles_amount_deserialize() {
        let yaml = "4t";
        let c: CyclesAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.0, 4_000_000_000_000);

        let yaml = "5000000000000";
        let c: CyclesAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.0, 5_000_000_000_000);
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
