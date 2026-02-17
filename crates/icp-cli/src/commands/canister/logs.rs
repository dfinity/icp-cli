use anyhow::{Context as _, anyhow};
use clap::Args;
use ic_management_canister_types::{CanisterLogRecord, FetchCanisterLogsResult};
use icp::context::Context;
use icp::signal::stop_signal;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::select;

use crate::commands::args;

/// Fetch and display canister logs
#[derive(Debug, Args)]
pub(crate) struct LogsArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Continuously fetch and display new logs until interrupted with Ctrl+C
    #[arg(long)]
    pub(crate) follow: bool,

    /// Polling interval in seconds when following logs (requires --follow)
    #[arg(long, requires = "follow", default_value = "2")]
    pub(crate) interval: u64,
}

pub(crate) async fn exec(ctx: &Context, args: &LogsArgs) -> Result<(), anyhow::Error> {
    // Validate interval
    if args.interval < 1 {
        return Err(anyhow!("Interval must be at least 1 second"));
    }

    let selections = args.cmd_args.selections();

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let canister_id = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    if args.follow {
        // Follow mode: continuously fetch and display new logs
        follow_logs(ctx, &mgmt, &canister_id, args.interval).await
    } else {
        // Single fetch mode: fetch all logs once
        fetch_and_display_logs(ctx, &mgmt, &canister_id).await
    }
}

async fn fetch_and_display_logs(
    ctx: &Context,
    mgmt: &ic_utils::interfaces::ManagementCanister<'_>,
    canister_id: &candid::Principal,
) -> Result<(), anyhow::Error> {
    let (result,): (FetchCanisterLogsResult,) = mgmt
        .fetch_canister_logs(canister_id)
        .await
        .context("Failed to fetch canister logs")?;

    for log in result.canister_log_records {
        let formatted = format_log(&log);
        let _ = ctx.term.write_line(&formatted);
    }

    Ok(())
}

async fn follow_logs(
    ctx: &Context,
    mgmt: &ic_utils::interfaces::ManagementCanister<'_>,
    canister_id: &candid::Principal,
    interval_seconds: u64,
) -> Result<(), anyhow::Error> {
    let mut last_idx: Option<u64> = None;
    let interval = std::time::Duration::from_secs(interval_seconds);

    loop {
        // Fetch all logs
        let (result,): (FetchCanisterLogsResult,) = mgmt
            .fetch_canister_logs(canister_id)
            .await
            .context("Failed to fetch canister logs")?;

        // Filter to only new logs based on last_idx
        let new_logs: Vec<_> = result
            .canister_log_records
            .into_iter()
            .filter(|log| match last_idx {
                None => true, // First iteration, show all logs
                Some(idx) => log.idx > idx,
            })
            .collect();

        if !new_logs.is_empty() {
            for log in &new_logs {
                let formatted = format_log(log);
                let _ = ctx.term.write_line(&formatted);
            }
            // Update last_idx to the highest idx we've displayed
            if let Some(last_log) = new_logs.last() {
                last_idx = Some(last_log.idx);
            }
        }

        // Wait for interval or stop signal
        select! {
            _ = tokio::time::sleep(interval) => {
                // Continue loop
            }
            _ = stop_signal() => {
                // Gracefully exit on signal
                break;
            }
        }
    }

    Ok(())
}

fn format_log(log: &CanisterLogRecord) -> String {
    let timestamp = format_timestamp(log.timestamp_nanos);
    let content = format_content(&log.content);
    format!("[{}. {}]: {}", log.idx, timestamp, content)
}

fn format_timestamp(timestamp_nanos: u64) -> String {
    // Convert nanoseconds since Unix epoch to RFC3339 format
    let seconds = (timestamp_nanos / 1_000_000_000) as i64;
    let nanos = (timestamp_nanos % 1_000_000_000) as u32;

    match OffsetDateTime::from_unix_timestamp(seconds) {
        Ok(dt) => {
            // Create a new datetime with nanoseconds
            let dt_with_nanos = dt.replace_nanosecond(nanos).unwrap_or(dt);
            dt_with_nanos
                .format(&Rfc3339)
                .unwrap_or_else(|_| timestamp_nanos.to_string())
        }
        Err(_) => timestamp_nanos.to_string(),
    }
}

fn format_content(content: &[u8]) -> String {
    // Try to decode as UTF-8
    if let Ok(text) = std::str::from_utf8(content) {
        // Check if the debug representation contains problematic Unicode escapes
        let debug_repr = format!("{:?}", text);
        if debug_repr.contains("\\u{") {
            // Contains problematic Unicode, treat as binary
            format!("(bytes) 0x{}", hex::encode(content))
        } else {
            // Valid UTF-8 text
            text.to_string()
        }
    } else {
        // Invalid UTF-8, display as hex
        format!("(bytes) 0x{}", hex::encode(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_content_valid_utf8() {
        let content = b"Hello, World!";
        let formatted = format_content(content);
        assert_eq!(formatted, "Hello, World!");
    }

    #[test]
    fn test_format_content_binary() {
        let content = vec![0xc0, 0xff, 0xee];
        let formatted = format_content(&content);
        assert_eq!(formatted, "(bytes) 0xc0ffee");
    }

    #[test]
    fn test_format_content_problematic_unicode() {
        // Content that is valid UTF-8 but contains problematic characters
        let content = b"\x00\x01\x02";
        let formatted = format_content(content);
        assert!(formatted.starts_with("(bytes) 0x000102"));
    }

    #[test]
    fn test_format_timestamp() {
        // Test timestamp: 2024-01-01T10:00:00.123456789Z
        let nanos = 1704103200123456789u64;
        let formatted = format_timestamp(nanos);
        assert_eq!(formatted, "2024-01-01T10:00:00.123456789Z");
    }

    #[test]
    fn test_format_log() {
        let log = CanisterLogRecord {
            idx: 42,
            timestamp_nanos: 1704103200123456789,
            content: b"Test message".to_vec(),
        };
        let formatted = format_log(&log);
        assert_eq!(formatted, "[42. 2024-01-01T10:00:00.123456789Z]: Test message");
    }
}
