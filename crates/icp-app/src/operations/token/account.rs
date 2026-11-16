use std::{fmt, str::FromStr};

/// A ledger account in either of the two textual shapes users supply: a
/// 32-byte hex ICP-ledger account identifier, or an ICRC-1 account.
#[derive(Debug, Clone, Copy)]
pub enum FlexibleAccountId {
    Icrc1(icrc_ledger_types::icrc1::account::Account),
    IcpLedger(ic_ledger_types::AccountIdentifier),
}

impl FromStr for FlexibleAccountId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try parsing as ICP ledger account (hex string)
        if let Ok(bytes) = hex::decode(s) {
            if bytes.len() == 32 {
                return ic_ledger_types::AccountIdentifier::from_slice(&bytes)
                    .map(FlexibleAccountId::IcpLedger)
                    .map_err(|e| format!("Invalid ICP ledger account identifier {s}: {e}"));
            } else {
                return Err(format!("Invalid ICP ledger account hex string: {s}"));
            }
        }
        // Try parsing as ICRC1 account
        if let Ok(account) = s.parse::<icrc_ledger_types::icrc1::account::Account>() {
            return Ok(FlexibleAccountId::Icrc1(account));
        }

        Err(format!("Invalid principal / account identifier: {s}"))
    }
}

impl fmt::Display for FlexibleAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlexibleAccountId::Icrc1(account) => account.fmt(f),
            FlexibleAccountId::IcpLedger(bytes) => hex::encode(bytes).fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icp_ledger_account_with_bad_checksum_is_an_error() {
        let valid = ic_ledger_types::AccountIdentifier::new(
            &candid::Principal::anonymous(),
            &ic_ledger_types::DEFAULT_SUBACCOUNT,
        );
        let mut bytes = valid.as_ref().to_vec();
        bytes[0] ^= 0xff;

        assert!(hex::encode(&bytes).parse::<FlexibleAccountId>().is_err());
        assert!(matches!(
            valid.to_hex().parse::<FlexibleAccountId>(),
            Ok(FlexibleAccountId::IcpLedger(a)) if a == valid
        ));
    }
}
