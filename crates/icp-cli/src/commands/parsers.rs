use icp_app::operations::cycles_purchase::{Currency, Money};

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

/// A decimal USD amount for `--amount`: `10`, `12.50`.
pub(crate) fn parse_usd_amount(input: &str) -> Result<Money, String> {
    Money::parse_decimal(input, Currency::Usd).map_err(|e| e.to_string())
}
