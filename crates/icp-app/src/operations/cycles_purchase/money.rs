//! A sum of money in a fiat currency, kept in minor units (cents) so no
//! arithmetic ever rounds.
//!
//! The gateway sells for USD only today, but nothing here assumes it: a new
//! currency is a new [`Currency`] variant, and the one place that encodes a
//! [`Money`] for the gateway matches on it exhaustively.

use std::fmt;
use std::str::FromStr;

use candid::Nat;
use num_traits::ToPrimitive;
use serde::{Serialize, Serializer};
use snafu::Snafu;

/// A fiat currency, by ISO 4217 code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Currency {
    Usd,
}

impl Currency {
    /// The ISO 4217 code, e.g. `USD`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Usd => "USD",
        }
    }

    /// Decimal places in the major unit: `2` for a currency with cents.
    pub fn exponent(self) -> u32 {
        match self {
            Self::Usd => 2,
        }
    }

    fn minor_per_major(self) -> u64 {
        10u64.pow(self.exponent())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Currency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            "USD" => Ok(Self::Usd),
            other => Err(format!(
                "unsupported currency '{other}'; only USD is supported"
            )),
        }
    }
}

/// Why a decimal amount would not parse as [`Money`].
#[derive(Debug, Snafu)]
pub enum ParseMoneyError {
    #[snafu(display("amount '{input}' is not a decimal number"))]
    NotDecimal { input: String },

    #[snafu(display("amount '{input}' has more than {max} decimal places"))]
    TooManyDecimals { input: String, max: u32 },

    #[snafu(display("amount must be greater than zero"))]
    Zero,

    #[snafu(display("amount '{input}' is too large"))]
    Overflow { input: String },
}

/// An amount of a [`Currency`], in its minor unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Money {
    pub minor_units: u64,
    pub currency: Currency,
}

impl Money {
    pub fn from_minor_units(minor_units: u64, currency: Currency) -> Self {
        Self {
            minor_units,
            currency,
        }
    }

    /// A gateway-reported amount in minor units, or `None` when it does not
    /// fit in a `u64`.
    pub fn from_nat_minor_units(minor_units: &Nat, currency: Currency) -> Option<Self> {
        minor_units
            .0
            .to_u64()
            .map(|minor_units| Self::from_minor_units(minor_units, currency))
    }

    /// Parse a decimal amount in major units: `10`, `12.5`, `12.50`, `1_000`.
    ///
    /// Rejects a sign, more decimal places than the currency has, and zero.
    pub fn parse_decimal(input: &str, currency: Currency) -> Result<Self, ParseMoneyError> {
        let cleaned: String = input.trim().chars().filter(|c| *c != '_').collect();
        let (whole, fraction) = match cleaned.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (cleaned.as_str(), ""),
        };
        let is_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
        if cleaned.is_empty()
            || (whole.is_empty() && fraction.is_empty())
            || !is_digits(whole)
            || !is_digits(fraction)
        {
            return NotDecimalSnafu { input }.fail();
        }
        let exponent = currency.exponent();
        if fraction.len() > exponent as usize {
            return TooManyDecimalsSnafu {
                input,
                max: exponent,
            }
            .fail();
        }

        let whole: u64 = if whole.is_empty() {
            0
        } else {
            whole.parse().ok().context_overflow(input)?
        };
        let fraction: u64 = if fraction.is_empty() {
            0
        } else {
            let padded = format!("{fraction:0<width$}", width = exponent as usize);
            padded.parse().ok().context_overflow(input)?
        };
        let minor_units = whole
            .checked_mul(currency.minor_per_major())
            .and_then(|w| w.checked_add(fraction))
            .context_overflow(input)?;
        if minor_units == 0 {
            return ZeroSnafu.fail();
        }
        Ok(Self::from_minor_units(minor_units, currency))
    }

    /// The amount in major units with the currency's full precision: `10.00`.
    pub fn to_decimal_string(&self) -> String {
        let per_major = self.currency.minor_per_major();
        format!(
            "{}.{:0>width$}",
            self.minor_units / per_major,
            self.minor_units % per_major,
            width = self.currency.exponent() as usize
        )
    }
}

trait ContextOverflow<T> {
    fn context_overflow(self, input: &str) -> Result<T, ParseMoneyError>;
}

impl<T> ContextOverflow<T> for Option<T> {
    fn context_overflow(self, input: &str) -> Result<T, ParseMoneyError> {
        self.ok_or_else(|| ParseMoneyError::Overflow {
            input: input.to_owned(),
        })
    }
}

/// `10.00 USD`: amount then code, as token amounts print `... ICP`.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal_string(), self.currency)
    }
}

/// `{"currency":"USD","value":"10.00"}`: the value is a string so no JSON
/// reader turns it into a float.
impl Serialize for Money {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct MoneyJson<'a> {
            currency: &'a str,
            value: String,
        }
        MoneyJson {
            currency: self.currency.code(),
            value: self.to_decimal_string(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(input: &str) -> Result<Money, ParseMoneyError> {
        Money::parse_decimal(input, Currency::Usd)
    }

    #[test]
    fn parses_whole_and_fractional_usd() {
        assert_eq!(usd("10").unwrap().minor_units, 1000);
        assert_eq!(usd("12.5").unwrap().minor_units, 1250);
        assert_eq!(usd("12.50").unwrap().minor_units, 1250);
        assert_eq!(usd("0.01").unwrap().minor_units, 1);
        assert_eq!(usd(".5").unwrap().minor_units, 50);
        assert_eq!(usd("1_000.00").unwrap().minor_units, 100_000);
        assert_eq!(usd(" 7 ").unwrap().minor_units, 700);
    }

    #[test]
    fn rejects_bad_amounts() {
        assert!(matches!(
            usd("10.005"),
            Err(ParseMoneyError::TooManyDecimals { max: 2, .. })
        ));
        assert!(matches!(usd("-1"), Err(ParseMoneyError::NotDecimal { .. })));
        assert!(matches!(
            usd("abc"),
            Err(ParseMoneyError::NotDecimal { .. })
        ));
        assert!(matches!(usd(""), Err(ParseMoneyError::NotDecimal { .. })));
        assert!(matches!(usd("."), Err(ParseMoneyError::NotDecimal { .. })));
        assert!(matches!(
            usd("1.2.3"),
            Err(ParseMoneyError::NotDecimal { .. })
        ));
        assert!(matches!(usd("0"), Err(ParseMoneyError::Zero)));
        assert!(matches!(usd("0.00"), Err(ParseMoneyError::Zero)));
        assert!(matches!(
            usd("99999999999999999999"),
            Err(ParseMoneyError::Overflow { .. })
        ));
    }

    #[test]
    fn displays_and_serializes_money() {
        let ten = Money::from_minor_units(1000, Currency::Usd);
        assert_eq!(ten.to_string(), "10.00 USD");
        assert_eq!(
            serde_json::to_string(&ten).unwrap(),
            r#"{"currency":"USD","value":"10.00"}"#
        );
        assert_eq!(
            Money::from_minor_units(5, Currency::Usd).to_string(),
            "0.05 USD"
        );
        assert_eq!(
            Money::from_minor_units(123_456, Currency::Usd).to_decimal_string(),
            "1234.56"
        );
    }

    #[test]
    fn nat_conversion_bounds() {
        assert_eq!(
            Money::from_nat_minor_units(&Nat::from(59u8), Currency::Usd),
            Some(Money::from_minor_units(59, Currency::Usd))
        );
        assert_eq!(
            Money::from_nat_minor_units(&Nat::from(u128::MAX), Currency::Usd),
            None
        );
    }

    #[test]
    fn currency_codes() {
        assert_eq!("usd".parse::<Currency>().unwrap(), Currency::Usd);
        assert!("EUR".parse::<Currency>().is_err());
        assert_eq!(Currency::Usd.to_string(), "USD");
    }
}
