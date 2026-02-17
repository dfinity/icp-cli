//! Miscellaneous utilities that don't belong to specific commands.

use anyhow::{Context as _, Result, bail};
use candid::IDLArgs;
use candid_parser::parse_idl_args;
use icp::{fs, manifest::InitArgsFormat, prelude::*};
use time::{OffsetDateTime, macros::format_description};

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
#[derive(Debug)]
pub(crate) enum ParsedArguments {
    /// Hex-encoded bytes
    Hex(Vec<u8>),
    /// Parsed Candid text format
    Candid(IDLArgs),
}

/// Parses arguments from a string that can be either:
/// - Hex-encoded bytes (if valid hex)
/// - Candid text format (if valid Candid)
/// - File path (if neither hex nor Candid)
pub(crate) fn parse_args<P: AsRef<Path> + ?Sized>(
    args_str: &str,
    base_path: &P,
) -> Result<ParsedArguments> {
    // Try to decode as hex first
    if let Ok(bytes) = hex::decode(args_str) {
        return Ok(ParsedArguments::Hex(bytes));
    }

    // Try to parse as Candid text format
    if let Ok(args) = parse_idl_args(args_str) {
        return Ok(ParsedArguments::Candid(args));
    }

    // If neither hex nor Candid, try to read as a file path
    let file_path = base_path.as_ref().join(args_str);
    if file_path.is_file()
        && let Ok(contents) = fs::read_to_string(&file_path)
    {
        // Recursively parse the file contents
        return parse_args(contents.trim(), base_path);
    }

    // If all attempts failed, return an error
    bail!(
        "Failed to parse arguments as hex, Candid literal, or as path to existing file: '{}'",
        args_str
    )
}

/// Resolve CLI-provided args (from `--args` / `--args-format` flags) into raw bytes.
pub(crate) fn resolve_cli_args(
    args_str: &str,
    format: Option<&InitArgsFormat>,
    base_path: &Path,
) -> Result<Vec<u8>> {
    match format {
        None => match parse_args(args_str, base_path)? {
            ParsedArguments::Hex(bytes) => Ok(bytes),
            ParsedArguments::Candid(args) => args
                .to_bytes()
                .context("Failed to encode Candid args to bytes"),
        },
        Some(InitArgsFormat::Bin) => {
            let file_path = base_path.join(args_str);
            Ok(fs::read(&file_path)?)
        }
        Some(InitArgsFormat::Hex) => {
            if let Ok(bytes) = hex::decode(args_str) {
                return Ok(bytes);
            }
            let file_path = base_path.join(args_str);
            let contents = fs::read_to_string(&file_path)?;
            hex::decode(contents.trim()).context("Failed to decode hex from file")
        }
        Some(InitArgsFormat::Idl) => {
            if let Ok(args) = parse_idl_args(args_str) {
                return args
                    .to_bytes()
                    .context("Failed to encode Candid args to bytes");
            }
            let file_path = base_path.join(args_str);
            let contents = fs::read_to_string(&file_path)?;
            let args =
                parse_idl_args(contents.trim()).context("Failed to parse Candid from file")?;
            args.to_bytes()
                .context("Failed to encode Candid args to bytes")
        }
    }
}

/// Format a nanosecond timestamp as a human-readable UTC datetime string.
pub(crate) fn format_timestamp(nanos: u64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp_nanos(nanos as i128) else {
        return nanos.to_string();
    };
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    datetime
        .format(&format)
        .unwrap_or_else(|_| nanos.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino_tempfile::Utf8TempDir;

    #[test]
    fn test_parse_args_hex() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let hex_str = "4449444c00";
        let result = parse_args(hex_str, &temp_dir.path()).unwrap();
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
        let temp_dir = Utf8TempDir::new().unwrap();
        let candid_str = "(42)";
        let result = parse_args(candid_str, &temp_dir.path()).unwrap();
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
        let temp_dir = Utf8TempDir::new().unwrap();
        let candid_str = r#"("test")"#;
        let result = parse_args(candid_str, &temp_dir.path()).unwrap();
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
        let temp_dir = Utf8TempDir::new().unwrap();
        let invalid_str = "not valid hex or candid";
        let result = parse_args(invalid_str, &temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_args_file_with_candid() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let file_path = temp_dir.path().join("args.txt");
        std::fs::write(file_path.as_std_path(), "(42)").unwrap();

        let result = parse_args("args.txt", &temp_dir.path()).unwrap();

        match result {
            ParsedArguments::Candid(args) => {
                let bytes = args.to_bytes().unwrap();
                assert_eq!(bytes, hex::decode("4449444c00017c2a").unwrap());
            }
            ParsedArguments::Hex(_) => {
                panic!("Expected Candid args, got hex bytes");
            }
        }
    }

    #[test]
    fn test_parse_args_file_with_hex() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let file_path = temp_dir.path().join("args_hex.txt");
        std::fs::write(file_path.as_std_path(), "4449444c00").unwrap();

        let result = parse_args("args_hex.txt", &temp_dir.path()).unwrap();

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
    fn test_parse_args_file_not_found() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let result = parse_args("nonexistent_file.txt", &temp_dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to parse arguments")
        );
    }

    #[test]
    fn test_parse_args_with_base_path() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let file_path = temp_dir.path().join("args.txt");
        std::fs::write(file_path.as_std_path(), "(42)").unwrap();

        // Test with relative path and base path
        let result = parse_args("args.txt", &temp_dir.path()).unwrap();

        match result {
            ParsedArguments::Candid(args) => {
                let bytes = args.to_bytes().unwrap();
                // Should successfully parse and encode
                assert_eq!(bytes, hex::decode("4449444c00017c2a").unwrap());
            }
            ParsedArguments::Hex(_) => {
                panic!("Expected Candid args, got hex bytes");
            }
        }
    }

    #[test]
    fn test_parse_init_args_hex() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let hex_str = "4449444c00";
        let result = match parse_args(hex_str, &temp_dir.path()).unwrap() {
            ParsedArguments::Hex(bytes) => bytes,
            ParsedArguments::Candid(args) => args.to_bytes().unwrap(),
        };
        assert_eq!(result, vec![0x44, 0x49, 0x44, 0x4c, 0x00]);
    }

    #[test]
    fn test_parse_init_args_candid_text() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let candid_str = "(42)";
        let result = match parse_args(candid_str, &temp_dir.path()).unwrap() {
            ParsedArguments::Hex(bytes) => bytes,
            ParsedArguments::Candid(args) => args.to_bytes().unwrap(),
        };
        // Expected bytes from: didc encode '(42)'
        assert_eq!(result, hex::decode("4449444c00017c2a").unwrap());
    }

    #[test]
    fn test_parse_init_args_invalid() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let invalid_str = "not valid hex or candid";
        let result = parse_args(invalid_str, &temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_init_args_from_file() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let file_path = temp_dir.path().join("args.txt");
        std::fs::write(file_path.as_std_path(), "(42)").unwrap();

        let result = match parse_args("args.txt", &temp_dir.path()).unwrap() {
            ParsedArguments::Hex(bytes) => bytes,
            ParsedArguments::Candid(args) => args.to_bytes().unwrap(),
        };

        // Expected bytes from: didc encode '(42)'
        assert_eq!(result, hex::decode("4449444c00017c2a").unwrap());
    }

    #[test]
    fn test_parse_init_args_with_base_path_from_file() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let file_path = temp_dir.path().join("init_args.txt");
        std::fs::write(file_path.as_std_path(), "4449444c00").unwrap();

        let result = match parse_args("init_args.txt", &temp_dir.path()).unwrap() {
            ParsedArguments::Hex(bytes) => bytes,
            ParsedArguments::Candid(args) => args.to_bytes().unwrap(),
        };
        assert_eq!(result, vec![0x44, 0x49, 0x44, 0x4c, 0x00]);
    }

    // --- resolve_cli_args tests ---

    #[test]
    fn test_resolve_cli_args_auto_detect() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let result = resolve_cli_args("(42)", None, temp_dir.path()).unwrap();
        assert_eq!(result, hex::decode("4449444c00017c2a").unwrap());
    }

    #[test]
    fn test_resolve_cli_args_bin_file() {
        let temp_dir = Utf8TempDir::new().unwrap();
        let raw_bytes = vec![0x44, 0x49, 0x44, 0x4c, 0x00];
        std::fs::write(temp_dir.path().join("args.bin").as_std_path(), &raw_bytes).unwrap();

        let result =
            resolve_cli_args("args.bin", Some(&InitArgsFormat::Bin), temp_dir.path()).unwrap();
        assert_eq!(result, raw_bytes);
    }

    #[test]
    fn test_resolve_cli_args_explicit_format_file_fallback() {
        let temp_dir = Utf8TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("args.hex").as_std_path(), "4449444c00").unwrap();

        let result =
            resolve_cli_args("args.hex", Some(&InitArgsFormat::Hex), temp_dir.path()).unwrap();
        assert_eq!(result, vec![0x44, 0x49, 0x44, 0x4c, 0x00]);
    }
}
