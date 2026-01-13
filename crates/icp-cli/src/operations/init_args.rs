use anyhow::{Context, Result};
use candid_parser::parse_idl_args;

/// Parses init args from a string that can be either:
/// - Hex-encoded bytes (if valid hex)
/// - Candid text format
pub(crate) fn parse_init_args(init_args_str: &str) -> Result<Vec<u8>> {
    // Try to decode as hex first
    if let Ok(bytes) = hex::decode(init_args_str) {
        return Ok(bytes);
    }

    // Otherwise, parse as Candid text format
    let args =
        parse_idl_args(init_args_str).context("Failed to parse init_args as Candid text format")?;

    args.to_bytes()
        .context("Failed to encode Candid args to bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex() {
        let hex_str = "4449444c00";
        let result = parse_init_args(hex_str).unwrap();
        assert_eq!(result, vec![0x44, 0x49, 0x44, 0x4c, 0x00]);
    }

    #[test]
    fn test_parse_candid_text() {
        let candid_str = "(42)";
        let result = parse_init_args(candid_str).unwrap();
        // Expected bytes from: didc encode '(42)'
        assert_eq!(result, hex::decode("4449444c00017c2a").unwrap());
    }

    #[test]
    fn test_parse_invalid() {
        let invalid_str = "not valid hex or candid";
        let result = parse_init_args(invalid_str);
        assert!(result.is_err());
    }
}
