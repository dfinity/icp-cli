//! Miscellaneous utilities that don't belong to specific commands.

use anyhow::{Context, Result};
use candid::IDLArgs;
use candid_parser::parse_idl_args;

pub async fn fetch_canister_metadata(
    agent: &ic_agent::Agent,
    canister_id: candid::Principal,
    metadata: &str,
) -> Option<String> {
    Some(
        String::from_utf8_lossy(
            &agent
                .read_state_canister_metadata(canister_id, metadata)
                .await
                .ok()?,
        )
        .into(),
    )
}

/// Result of parsing arguments that can be either hex or Candid format
pub(crate) enum ParsedArguments {
    /// Hex-encoded bytes (already in Candid binary format)
    Hex(Vec<u8>),
    /// Parsed Candid text format
    Candid(IDLArgs),
}

/// Parses arguments from a string that can be either:
/// - Hex-encoded bytes (if valid hex)
/// - Candid text format
pub(crate) fn parse_args(args_str: &str) -> Result<ParsedArguments> {
    // Try to decode as hex first
    if let Ok(bytes) = hex::decode(args_str) {
        return Ok(ParsedArguments::Hex(bytes));
    }

    // Otherwise, parse as Candid text format
    let args =
        parse_idl_args(args_str).context("Failed to parse arguments as hex or Candid literal")?;

    Ok(ParsedArguments::Candid(args))
}

/// Parses init args from a string and converts to bytes.
/// This is a convenience wrapper around parse_args that always returns bytes.
/// Use this if you won't have Candid types available.
pub(crate) fn parse_init_args(init_args_str: &str) -> Result<Vec<u8>> {
    match parse_args(init_args_str)? {
        ParsedArguments::Hex(bytes) => Ok(bytes),
        ParsedArguments::Candid(args) => args
            .to_bytes()
            .context("Failed to encode Candid args to bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_hex() {
        let hex_str = "4449444c00";
        let result = parse_args(hex_str).unwrap();
        match result {
            ParsedArguments::Hex(bytes) => {
                assert_eq!(bytes, vec![0x44, 0x49, 0x44, 0x4c, 0x00]);
            }
            ParsedArguments::Candid(_) => {
                panic!("Expected hex bytes, got Candid args");
            }
        }
    }

    #[test]
    fn test_parse_args_candid_text() {
        let candid_str = "(42)";
        let result = parse_args(candid_str).unwrap();
        match result {
            ParsedArguments::Candid(args) => {
                // Expected bytes from: didc encode '(42)'
                let bytes = args.to_bytes().unwrap();
                assert_eq!(bytes, hex::decode("4449444c00017c2a").unwrap());
            }
            ParsedArguments::Hex(_) => {
                panic!("Expected Candid args, got hex bytes");
            }
        }
    }

    #[test]
    fn test_parse_args_string() {
        let candid_str = r#"("test")"#;
        let result = parse_args(candid_str).unwrap();
        match result {
            ParsedArguments::Candid(args) => {
                let bytes = args.to_bytes().unwrap();
                // Expected bytes from: didc encode '("test")'
                assert_eq!(bytes, hex::decode("4449444c0001710474657374").unwrap());
            }
            ParsedArguments::Hex(_) => {
                panic!("Expected Candid args, got hex bytes");
            }
        }
    }

    #[test]
    fn test_parse_args_invalid() {
        let invalid_str = "not valid hex or candid";
        let result = parse_args(invalid_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_init_args_hex() {
        let hex_str = "4449444c00";
        let result = parse_init_args(hex_str).unwrap();
        assert_eq!(result, vec![0x44, 0x49, 0x44, 0x4c, 0x00]);
    }

    #[test]
    fn test_parse_init_args_candid_text() {
        let candid_str = "(42)";
        let result = parse_init_args(candid_str).unwrap();
        // Expected bytes from: didc encode '(42)'
        assert_eq!(result, hex::decode("4449444c00017c2a").unwrap());
    }

    #[test]
    fn test_parse_init_args_invalid() {
        let invalid_str = "not valid hex or candid";
        let result = parse_init_args(invalid_str);
        assert!(result.is_err());
    }
}
