use anyhow::{Context as _, anyhow};
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use ic_utils::interfaces::management_canister::{
    CanisterLogFilter, CanisterLogRecord, FetchCanisterLogsArgs, FetchCanisterLogsResult,
};
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
    #[arg(short, long)]
    pub(crate) follow: bool,

    /// Polling interval in seconds when following logs (requires --follow)
    #[arg(long, requires = "follow", default_value = "2")]
    pub(crate) interval: u64,

    /// Show logs at or after this timestamp. Accepts nanoseconds since Unix epoch or RFC3339
    /// (e.g. '2024-01-01T00:00:00Z')
    #[arg(long, value_name = "TIMESTAMP", conflicts_with_all = ["since_index", "until_index"], value_parser = parse_timestamp)]
    pub(crate) since: Option<u64>,

    /// Show logs at or before this timestamp. Accepts nanoseconds since Unix epoch or RFC3339
    /// (e.g. '2024-01-01T00:00:00Z'). Requires --since
    #[arg(long, value_name = "TIMESTAMP", requires = "since", conflicts_with_all = ["since_index", "until_index"], value_parser = parse_timestamp)]
    pub(crate) until: Option<u64>,

    /// Show logs at or after this log index (inclusive)
    #[arg(long, value_name = "INDEX", conflicts_with_all = ["since", "until"])]
    pub(crate) since_index: Option<u64>,

    /// Show logs at or before this log index (inclusive). Requires --since-index
    #[arg(long, value_name = "INDEX", requires = "since_index", conflicts_with_all = ["since", "until"])]
    pub(crate) until_index: Option<u64>,
}

fn parse_timestamp(s: &str) -> Result<u64, String> {
    // Try raw nanoseconds first
    if let Ok(nanos) = s.parse::<u64>() {
        return Ok(nanos);
    }
    // Fall back to RFC3339
    OffsetDateTime::parse(s, &Rfc3339)
        .map(|dt| {
            let nanos_per_sec = 1_000_000_000u64;
            (dt.unix_timestamp() as u64) * nanos_per_sec + dt.nanosecond() as u64
        })
        .map_err(|_| format!("'{s}' is not a valid nanosecond timestamp or RFC3339 datetime"))
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

    let mgmt = ManagementCanister::create(&agent);

    let initial_filter = build_filter(args);

    if args.follow {
        // Follow mode: continuously fetch and display new logs
        follow_logs(ctx, &mgmt, &canister_id, args.interval, initial_filter).await
    } else {
        // Single fetch mode: fetch all logs once
        fetch_and_display_logs(ctx, &mgmt, &canister_id, initial_filter).await
    }
}

fn build_filter(args: &LogsArgs) -> Option<CanisterLogFilter> {
    if let Some(start) = args.since_index {
        Some(CanisterLogFilter::ByIdx {
            start,
            end: args.until_index.unwrap_or(u64::MAX),
        })
    } else if let Some(start) = args.since {
        Some(CanisterLogFilter::ByTimestampNanos {
            start,
            end: args.until.unwrap_or(u64::MAX),
        })
    } else {
        None
    }
}

async fn fetch_and_display_logs(
    ctx: &Context,
    mgmt: &ManagementCanister<'_>,
    canister_id: &candid::Principal,
    filter: Option<CanisterLogFilter>,
) -> Result<(), anyhow::Error> {
    let fetch_args = FetchCanisterLogsArgs {
        canister_id: *canister_id,
        filter,
    };
    let (result,): (FetchCanisterLogsResult,) = mgmt
        .fetch_canister_logs(&fetch_args)
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
    mgmt: &ManagementCanister<'_>,
    canister_id: &candid::Principal,
    interval_seconds: u64,
    initial_filter: Option<CanisterLogFilter>,
) -> Result<(), anyhow::Error> {
    let mut last_idx: Option<u64> = None;
    let interval = std::time::Duration::from_secs(interval_seconds);

    loop {
        // On first iteration use the user-supplied filter; on subsequent iterations use
        // server-side idx filtering to fetch only new logs.
        let filter = match last_idx {
            Some(idx) => Some(CanisterLogFilter::ByIdx {
                start: idx + 1,
                end: u64::MAX,
            }),
            None => initial_filter.clone(),
        };
        let fetch_args = FetchCanisterLogsArgs {
            canister_id: *canister_id,
            filter,
        };
        let (result,): (FetchCanisterLogsResult,) = mgmt
            .fetch_canister_logs(&fetch_args)
            .await
            .context("Failed to fetch canister logs")?;

        let new_logs = result.canister_log_records;

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
    let seconds = (timestamp_nanos / 1_000_000_000) as i64;
    let nanos = (timestamp_nanos % 1_000_000_000) as u32;

    match OffsetDateTime::from_unix_timestamp(seconds) {
        Ok(dt) => {
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
        // Check if all characters are printable (excluding control characters)
        if text
            .chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\t')
        {
            // Valid UTF-8 text with only printable characters
            text.to_string()
        } else {
            // Contains control characters, treat as binary
            format!("(bytes) 0x{}", hex::encode(content))
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
        assert_eq!(
            formatted,
            "[42. 2024-01-01T10:00:00.123456789Z]: Test message"
        );
    }

    #[test]
    fn test_parse_timestamp_raw_nanos() {
        assert_eq!(
            parse_timestamp("1704103200123456789"),
            Ok(1704103200123456789)
        );
        assert_eq!(parse_timestamp("0"), Ok(0));
    }

    #[test]
    fn test_parse_timestamp_rfc3339() {
        // 2024-01-01T10:00:00Z = 1704103200000000000 nanos
        assert_eq!(
            parse_timestamp("2024-01-01T10:00:00Z"),
            Ok(1704103200_000_000_000)
        );
    }

    #[test]
    fn test_parse_timestamp_rfc3339_with_nanos() {
        assert_eq!(
            parse_timestamp("2024-01-01T10:00:00.123456789Z"),
            Ok(1704103200123456789)
        );
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        assert!(parse_timestamp("not-a-timestamp").is_err());
    }
}
