//! Parsing of token, cycle, memory, and duration amounts with support for suffixes and underscores.

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

fn parse_cycles_str(s: &str) -> Result<u128, String> {
    let token_amount = parse_token_amount(s)?;
    let unit_amount = to_token_unit_amount(token_amount, 0)?;
    unit_amount
        .to_u128()
        .ok_or_else(|| format!("Cycles amount too large: '{}'", s))
}

/// An amount of cycles.
///
/// Deserializes from a number or a string with suffixes (k, m, b, t) and optional underscore separators.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema)]
#[schemars(untagged)]
pub enum CyclesAmount {
    Number(u64), // yaml only supports up to u64
    Str(String),
}

impl CyclesAmount {
    pub fn get(&self) -> u128 {
        match self {
            CyclesAmount::Number(n) => *n as u128,
            CyclesAmount::Str(s) => parse_cycles_str(s)
                .unwrap_or_else(|e| panic!("invalid cycles amount '{}': {}", s, e)),
        }
    }
}

impl<'de> Deserialize<'de> for CyclesAmount {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Identical enum to CyclesAmount. Needed to avoid a circular dependency.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(u64),
            Str(String),
        }
        let v = Raw::deserialize(d).map_err(|_| {
            serde::de::Error::custom("cycles amount must be a number or a string with optional suffix (k, m, b, t), e.g. 1000 or \"4t\"")
        })?;
        let c = match v {
            Raw::Number(n) => CyclesAmount::Number(n),
            Raw::Str(ref s) => {
                parse_cycles_str(s).map_err(serde::de::Error::custom)?; // validate the string is a valid cycles amount
                CyclesAmount::Str(s.clone())
            }
        };
        Ok(c)
    }
}

impl Serialize for CyclesAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            CyclesAmount::Number(n) => serializer.serialize_u64(*n),
            CyclesAmount::Str(s) => serializer.serialize_str(s),
        }
    }
}

impl FromStr for CyclesAmount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_cycles_str(s)?; // validate the string is a valid cycles amount
        Ok(CyclesAmount::Str(s.to_string()))
    }
}

impl fmt::Display for CyclesAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl From<CyclesAmount> for u128 {
    fn from(c: CyclesAmount) -> Self {
        c.get()
    }
}

impl From<u128> for CyclesAmount {
    fn from(n: u128) -> Self {
        if let Ok(n64) = u64::try_from(n) {
            CyclesAmount::Number(n64)
        } else {
            CyclesAmount::Str(n.to_string())
        }
    }
}

const KB: u64 = 1000;
const KIB: u64 = 1024;
const MB: u64 = 1_000_000;
const MIB: u64 = 1024 * 1024;
const GB: u64 = 1_000_000_000;
const GIB: u64 = 1024 * 1024 * 1024;

fn parse_memory_str(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Memory amount cannot be empty".to_string());
    }
    let lower = s.to_lowercase();
    let (number_part, factor) = if lower.ends_with("gib") {
        (&s[..s.len() - 3], GIB)
    } else if lower.ends_with("gb") {
        (&s[..s.len() - 2], GB)
    } else if lower.ends_with("mib") {
        (&s[..s.len() - 3], MIB)
    } else if lower.ends_with("mb") {
        (&s[..s.len() - 2], MB)
    } else if lower.ends_with("kib") {
        (&s[..s.len() - 3], KIB)
    } else if lower.ends_with("kb") {
        (&s[..s.len() - 2], KB)
    } else {
        (s, 1u64)
    };
    let cleaned = number_part.trim().replace('_', "");
    let amount =
        BigDecimal::from_str(&cleaned).map_err(|_| format!("Invalid memory amount: '{}'", s))?;
    if amount.is_negative() {
        return Err(format!("Memory amount cannot be negative: '{}'", s));
    }
    let product = amount * BigDecimal::from(factor);
    if !product.is_integer() {
        return Err(
            "Memory amount must be a whole number of bytes (fractional bytes not allowed)"
                .to_string(),
        );
    }
    product
        .to_u64()
        .ok_or_else(|| format!("Memory amount too large: '{}'", s))
}

/// An amount of memory in bytes.
///
/// Deserializes from a number or a string with suffixes (kb, kib, mb, mib, gb, gib),
/// optional decimals, and optional underscore separators.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema)]
#[schemars(untagged)]
pub enum MemoryAmount {
    Number(u64),
    Str(String),
}

impl MemoryAmount {
    pub fn get(&self) -> u64 {
        match self {
            MemoryAmount::Number(n) => *n,
            MemoryAmount::Str(s) => parse_memory_str(s)
                .unwrap_or_else(|e| panic!("invalid memory amount '{}': {}", s, e)),
        }
    }
}

impl<'de> Deserialize<'de> for MemoryAmount {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(u64),
            Str(String),
        }
        let v = Raw::deserialize(d).map_err(|_| {
            serde::de::Error::custom(
                "memory amount must be a number or a string with optional suffix (kb, kib, mb, mib, gb, gib), e.g. 1024 or \"2.5gib\"",
            )
        })?;
        let m = match v {
            Raw::Number(n) => MemoryAmount::Number(n),
            Raw::Str(ref s) => {
                parse_memory_str(s).map_err(serde::de::Error::custom)?;
                MemoryAmount::Str(s.clone())
            }
        };
        Ok(m)
    }
}

impl Serialize for MemoryAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            MemoryAmount::Number(n) => serializer.serialize_u64(*n),
            MemoryAmount::Str(s) => serializer.serialize_str(s),
        }
    }
}

impl FromStr for MemoryAmount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_memory_str(s)?;
        Ok(MemoryAmount::Str(s.to_string()))
    }
}

impl fmt::Display for MemoryAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl From<MemoryAmount> for u64 {
    fn from(m: MemoryAmount) -> Self {
        m.get()
    }
}

impl From<u64> for MemoryAmount {
    fn from(n: u64) -> Self {
        MemoryAmount::Number(n)
    }
}

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_DAY: u64 = 86400;
const SECONDS_PER_WEEK: u64 = 604800;

fn parse_duration_str(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Duration cannot be empty".to_string());
    }
    let lower = s.to_lowercase();
    let (number_part, factor) = if lower.ends_with('w') {
        (&s[..s.len() - 1], SECONDS_PER_WEEK)
    } else if lower.ends_with('d') {
        (&s[..s.len() - 1], SECONDS_PER_DAY)
    } else if lower.ends_with('h') {
        (&s[..s.len() - 1], SECONDS_PER_HOUR)
    } else if lower.ends_with('m') {
        (&s[..s.len() - 1], SECONDS_PER_MINUTE)
    } else if lower.ends_with('s') {
        (&s[..s.len() - 1], 1u64)
    } else {
        (s, 1u64)
    };
    let cleaned = number_part.trim().replace('_', "");
    if cleaned.is_empty() {
        return Err(format!("Invalid duration: '{s}'"));
    }
    let value: u64 = cleaned
        .parse()
        .map_err(|_| format!("Invalid duration: '{s}'"))?;
    value
        .checked_mul(factor)
        .ok_or_else(|| format!("Duration too large: '{s}'"))
}

/// A duration in seconds.
///
/// Deserializes from a number (seconds) or a string with duration suffix (s, m, h, d, w)
/// and optional underscore separators.
///
/// Suffixes (case-insensitive):
/// - `s` — seconds
/// - `m` — minutes (×60)
/// - `h` — hours (×3600)
/// - `d` — days (×86400)
/// - `w` — weeks (×604800)
///
/// A bare number without suffix is treated as seconds.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema)]
#[schemars(untagged)]
pub enum DurationAmount {
    Number(u64),
    Str(String),
}

impl DurationAmount {
    pub fn get(&self) -> u64 {
        match self {
            DurationAmount::Number(n) => *n,
            DurationAmount::Str(s) => {
                parse_duration_str(s).unwrap_or_else(|e| panic!("invalid duration '{}': {}", s, e))
            }
        }
    }
}

impl<'de> Deserialize<'de> for DurationAmount {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(u64),
            Str(String),
        }
        let v = Raw::deserialize(d).map_err(|_| {
            serde::de::Error::custom(
                "duration must be a number (seconds) or a string with optional suffix (s, m, h, d, w), e.g. 2592000 or \"30d\"",
            )
        })?;
        let c = match v {
            Raw::Number(n) => DurationAmount::Number(n),
            Raw::Str(ref s) => {
                parse_duration_str(s).map_err(serde::de::Error::custom)?;
                DurationAmount::Str(s.clone())
            }
        };
        Ok(c)
    }
}

impl Serialize for DurationAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            DurationAmount::Number(n) => serializer.serialize_u64(*n),
            DurationAmount::Str(s) => serializer.serialize_str(s),
        }
    }
}

impl FromStr for DurationAmount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_duration_str(s)?;
        Ok(DurationAmount::Str(s.to_string()))
    }
}

impl fmt::Display for DurationAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl From<DurationAmount> for u64 {
    fn from(d: DurationAmount) -> Self {
        d.get()
    }
}

impl From<u64> for DurationAmount {
    fn from(n: u64) -> Self {
        DurationAmount::Number(n)
    }
}

impl PartialEq<u64> for DurationAmount {
    fn eq(&self, other: &u64) -> bool {
        self.get() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_amount_from_str_plain() {
        assert_eq!("1".parse::<CyclesAmount>().unwrap().get(), 1);
        assert_eq!("1000".parse::<CyclesAmount>().unwrap().get(), 1000);
    }

    #[test]
    fn cycles_amount_from_str_suffixes() {
        assert_eq!("1k".parse::<CyclesAmount>().unwrap().get(), 1000);
        assert_eq!(
            "1t".parse::<CyclesAmount>().unwrap().get(),
            1_000_000_000_000
        );
        assert_eq!(
            "4t".parse::<CyclesAmount>().unwrap().get(),
            4_000_000_000_000
        );
        assert_eq!(
            "0.5t".parse::<CyclesAmount>().unwrap().get(),
            500_000_000_000
        );
    }

    #[test]
    fn cycles_amount_from_str_underscores() {
        assert_eq!("1_000".parse::<CyclesAmount>().unwrap().get(), 1000);
    }

    #[test]
    fn cycles_amount_from_str_fractional_rejected() {
        assert!("1.5".parse::<CyclesAmount>().is_err());
    }

    #[test]
    fn cycles_amount_deserialize() {
        let yaml = "4t";
        let c: CyclesAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.get(), 4_000_000_000_000);

        let yaml = "5000000000000";
        let c: CyclesAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.get(), 5_000_000_000_000);
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

    #[test]
    fn memory_amount_from_str_plain() {
        assert_eq!("1".parse::<MemoryAmount>().unwrap().get(), 1);
        assert_eq!("1024".parse::<MemoryAmount>().unwrap().get(), 1024);
    }

    #[test]
    fn memory_amount_from_str_suffixes() {
        assert_eq!("1kb".parse::<MemoryAmount>().unwrap().get(), 1000);
        assert_eq!("1kib".parse::<MemoryAmount>().unwrap().get(), 1024);
        assert_eq!("1mb".parse::<MemoryAmount>().unwrap().get(), 1_000_000);
        assert_eq!("1mib".parse::<MemoryAmount>().unwrap().get(), 1024 * 1024);
        assert_eq!("1gb".parse::<MemoryAmount>().unwrap().get(), 1_000_000_000);
        assert_eq!(
            "1gib".parse::<MemoryAmount>().unwrap().get(),
            1024 * 1024 * 1024
        );
        assert_eq!(
            "2 GiB".parse::<MemoryAmount>().unwrap().get(),
            2 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn memory_amount_from_str_decimals() {
        assert_eq!("0.5kib".parse::<MemoryAmount>().unwrap().get(), 512);
        assert_eq!("1.5gib".parse::<MemoryAmount>().unwrap().get(), 1610612736);
    }

    #[test]
    fn memory_amount_fractional_bytes_rejected() {
        assert!("1.5".parse::<MemoryAmount>().is_err()); // 1.5 bytes
        assert!("0.3kib".parse::<MemoryAmount>().is_err()); // 307.2 bytes
    }

    #[test]
    fn memory_amount_from_str_underscores() {
        assert_eq!("1_024".parse::<MemoryAmount>().unwrap().get(), 1024);
    }

    #[test]
    fn memory_amount_deserialize() {
        let yaml = "2gib";
        let m: MemoryAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(m.get(), 2 * 1024 * 1024 * 1024);

        let yaml = "4294967296";
        let m: MemoryAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(m.get(), 4294967296);
    }

    #[test]
    fn duration_amount_from_str_plain() {
        assert_eq!("60".parse::<DurationAmount>().unwrap().get(), 60);
        assert_eq!("2592000".parse::<DurationAmount>().unwrap().get(), 2592000);
    }

    #[test]
    fn duration_amount_from_str_underscores() {
        assert_eq!(
            "2_592_000".parse::<DurationAmount>().unwrap().get(),
            2592000
        );
    }

    #[test]
    fn duration_amount_from_str_suffixes() {
        assert_eq!("60s".parse::<DurationAmount>().unwrap().get(), 60);
        assert_eq!("90m".parse::<DurationAmount>().unwrap().get(), 5400);
        assert_eq!("24h".parse::<DurationAmount>().unwrap().get(), 86400);
        assert_eq!("30d".parse::<DurationAmount>().unwrap().get(), 2592000);
        assert_eq!("4w".parse::<DurationAmount>().unwrap().get(), 2419200);
    }

    #[test]
    fn duration_amount_from_str_case_insensitive() {
        assert_eq!("30D".parse::<DurationAmount>().unwrap().get(), 2592000);
        assert_eq!("1W".parse::<DurationAmount>().unwrap().get(), 604800);
        assert_eq!("24H".parse::<DurationAmount>().unwrap().get(), 86400);
        assert_eq!("60S".parse::<DurationAmount>().unwrap().get(), 60);
        assert_eq!("90M".parse::<DurationAmount>().unwrap().get(), 5400);
    }

    #[test]
    fn duration_amount_from_str_underscores_with_suffix() {
        assert_eq!(
            "2_592_000s".parse::<DurationAmount>().unwrap().get(),
            2592000
        );
    }

    #[test]
    fn duration_amount_from_str_errors() {
        assert!("abc".parse::<DurationAmount>().is_err());
        assert!("".parse::<DurationAmount>().is_err());
        assert!("1x".parse::<DurationAmount>().is_err());
        assert!("1.5d".parse::<DurationAmount>().is_err());
        assert!("-1d".parse::<DurationAmount>().is_err());
    }

    #[test]
    fn duration_amount_deserialize() {
        let yaml = "30d";
        let d: DurationAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(d.get(), 2592000);

        let yaml = "2592000";
        let d: DurationAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(d.get(), 2592000);

        let yaml = "2_592_000";
        let d: DurationAmount = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(d.get(), 2592000);
    }

    #[test]
    fn duration_amount_partial_eq_u64() {
        let d = DurationAmount::Number(2592000);
        assert!(d == 2592000);
        assert!(d != 0);

        let d = DurationAmount::Str("30d".to_string());
        assert!(d == 2592000);
    }
}
