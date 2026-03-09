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

    /// Show logs at or after this timestamp (inclusive). Accepts nanoseconds since Unix epoch or RFC3339
    /// (e.g. '2024-01-01T00:00:00Z'). Cannot be used with --follow
    #[arg(long, value_name = "TIMESTAMP", conflicts_with_all = ["follow", "since_index", "until_index"], value_parser = parse_timestamp)]
    pub(crate) since: Option<u64>,

    /// Show logs before this timestamp (exclusive). Accepts nanoseconds since Unix epoch or RFC3339
    /// (e.g. '2024-01-01T00:00:00Z'). Cannot be used with --follow
    #[arg(long, value_name = "TIMESTAMP", conflicts_with_all = ["follow", "since_index", "until_index"], value_parser = parse_timestamp)]
    pub(crate) until: Option<u64>,

    /// Show logs at or after this log index (inclusive). Cannot be used with --follow
    #[arg(long, value_name = "INDEX", conflicts_with_all = ["follow", "since", "until"])]
    pub(crate) since_index: Option<u64>,

    /// Show logs before this log index (exclusive). Cannot be used with --follow
    #[arg(long, value_name = "INDEX", conflicts_with_all = ["follow", "since", "until"])]
    pub(crate) until_index: Option<u64>,
}

fn parse_timestamp(s: &str) -> Result<u64, String> {
    // Try raw nanoseconds first
    if let Ok(nanos) = s.parse::<u64>() {
        return Ok(nanos);
    }
    // Detect numeric overflow before falling back to RFC3339
    if s.parse::<u128>().is_ok() {
        return Err(format!(
            "'{s}' overflows the nanosecond timestamp range (u64)"
        ));
    }
    // Fall back to RFC3339
    let dt = OffsetDateTime::parse(s, &Rfc3339)
        .map_err(|_| format!("'{s}' is not a valid nanosecond timestamp or RFC3339 datetime"))?;
    let nanos = dt.unix_timestamp_nanos();
    u64::try_from(nanos).map_err(|_| {
        if nanos < 0 {
            format!(
                "'{s}' is before the Unix epoch; timestamp must be a non-negative number \
                 or an RFC3339 datetime at or after 1970-01-01T00:00:00Z"
            )
        } else {
            format!("'{s}' overflows the nanosecond timestamp range (u64)")
        }
    })
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

    if args.follow {
        // Follow mode: continuously fetch and display new logs
        follow_logs(ctx, &mgmt, &canister_id, args.interval).await
    } else {
        // Single fetch mode: fetch all logs once
        fetch_and_display_logs(ctx, &mgmt, &canister_id, build_filter(args)?).await
    }
}

fn build_filter(args: &LogsArgs) -> Result<Option<CanisterLogFilter>, anyhow::Error> {
    if args.since_index.is_some() || args.until_index.is_some() {
        let start = args.since_index.unwrap_or(0);
        let end = args.until_index.unwrap_or(u64::MAX);
        if end == 0 {
            return Err(anyhow!(
                "--until-index must be greater than 0 (the end bound is exclusive)"
            ));
        }
        if start >= end {
            return Err(anyhow!(
                "--since-index ({start}) must be less than --until-index ({end})"
            ));
        }
        Ok(Some(CanisterLogFilter::ByIdx { start, end }))
    } else if args.since.is_some() || args.until.is_some() {
        let start = args.since.unwrap_or(0);
        let end = args.until.unwrap_or(u64::MAX);
        if end == 0 {
            return Err(anyhow!(
                "--until must be greater than 0 (the end bound is exclusive)"
            ));
        }
        if start >= end {
            return Err(anyhow!(
                "--since timestamp must be less than --until timestamp"
            ));
        }
        Ok(Some(CanisterLogFilter::ByTimestampNanos { start, end }))
    } else {
        Ok(None)
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

const FOLLOW_LOOKBACK_NANOS: u64 = 60 * 60 * 1_000_000_000; // 1 hour

async fn follow_logs(
    ctx: &Context,
    mgmt: &ManagementCanister<'_>,
    canister_id: &candid::Principal,
    interval_seconds: u64,
) -> Result<(), anyhow::Error> {
    let mut last_idx: Option<u64> = None;
    let interval = std::time::Duration::from_secs(interval_seconds);

    loop {
        let filter = match last_idx {
            Some(idx) => Some(CanisterLogFilter::ByIdx {
                start: idx + 1, // Start from the next log index after the last one we displayed
                end: u64::MAX,
            }),
            None => {
                // First fetch: look back 1 hour from now
                let now_nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| u64::try_from(d.as_nanos()).ok())
                    .unwrap_or(0);
                Some(CanisterLogFilter::ByTimestampNanos {
                    start: now_nanos.saturating_sub(FOLLOW_LOOKBACK_NANOS),
                    end: u64::MAX,
                })
            }
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

    /// 2024-01-01T10:00:00.123456789Z as nanoseconds since Unix epoch.
    const TEST_TIMESTAMP_NANOS: u64 = 1_704_103_200_123_456_789;
    const TEST_TIMESTAMP_RFC3339: &str = "2024-01-01T10:00:00.123456789Z";

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
        let formatted = format_timestamp(TEST_TIMESTAMP_NANOS);
        assert_eq!(formatted, TEST_TIMESTAMP_RFC3339);
    }

    #[test]
    fn test_format_log() {
        let log = CanisterLogRecord {
            idx: 42,
            timestamp_nanos: TEST_TIMESTAMP_NANOS,
            content: b"Test message".to_vec(),
        };
        let formatted = format_log(&log);
        assert_eq!(
            formatted,
            format!("[42. {TEST_TIMESTAMP_RFC3339}]: Test message")
        );
    }

    #[test]
    fn test_parse_timestamp_raw_nanos() {
        assert_eq!(
            parse_timestamp(&TEST_TIMESTAMP_NANOS.to_string()),
            Ok(TEST_TIMESTAMP_NANOS)
        );
        assert_eq!(parse_timestamp("0"), Ok(0));
    }

    #[test]
    fn test_parse_timestamp_rfc3339() {
        // 2024-01-01T10:00:00Z = 1704103200000000000 nanos
        assert_eq!(
            parse_timestamp("2024-01-01T10:00:00Z"),
            Ok(1_704_103_200_000_000_000)
        );
    }

    #[test]
    fn test_parse_timestamp_rfc3339_with_nanos() {
        assert_eq!(
            parse_timestamp(TEST_TIMESTAMP_RFC3339),
            Ok(TEST_TIMESTAMP_NANOS)
        );
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        assert!(parse_timestamp("not-a-timestamp").is_err());
    }

    #[test]
    fn test_parse_timestamp_numeric_overflow() {
        let result = parse_timestamp("99999999999999999999999");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("overflows the nanosecond timestamp range")
        );
    }

    #[test]
    fn test_parse_timestamp_before_epoch() {
        let result = parse_timestamp("1969-12-31T23:59:59Z");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("before the Unix epoch"));
    }

    fn make_logs_args(
        since: Option<u64>,
        until: Option<u64>,
        since_index: Option<u64>,
        until_index: Option<u64>,
    ) -> LogsArgs {
        LogsArgs {
            cmd_args: args::CanisterCommandArgs {
                canister: args::Canister::Name("test".to_string()),
                network: Default::default(),
                environment: Default::default(),
                identity: Default::default(),
            },
            follow: false,
            interval: 2,
            since,
            until,
            since_index,
            until_index,
        }
    }

    #[test]
    fn build_filter_no_flags() {
        let args = make_logs_args(None, None, None, None);
        assert!(build_filter(&args).unwrap().is_none());
    }

    #[test]
    fn build_filter_since_index_only() {
        let args = make_logs_args(None, None, Some(5), None);
        let filter = build_filter(&args).unwrap().unwrap();
        assert!(matches!(
            filter,
            CanisterLogFilter::ByIdx {
                start: 5,
                end: u64::MAX
            }
        ));
    }

    #[test]
    fn build_filter_until_index_only() {
        let args = make_logs_args(None, None, None, Some(10));
        let filter = build_filter(&args).unwrap().unwrap();
        assert!(matches!(
            filter,
            CanisterLogFilter::ByIdx { start: 0, end: 10 }
        ));
    }

    #[test]
    fn build_filter_both_indices() {
        let args = make_logs_args(None, None, Some(3), Some(7));
        let filter = build_filter(&args).unwrap().unwrap();
        assert!(matches!(
            filter,
            CanisterLogFilter::ByIdx { start: 3, end: 7 }
        ));
    }

    #[test]
    fn build_filter_inverted_indices_error() {
        let args = make_logs_args(None, None, Some(10), Some(5));
        let err = build_filter(&args).unwrap_err().to_string();
        assert!(err.contains("--since-index (10) must be less than --until-index (5)"));
    }

    #[test]
    fn build_filter_since_timestamp_only() {
        let args = make_logs_args(Some(1000), None, None, None);
        let filter = build_filter(&args).unwrap().unwrap();
        assert!(matches!(
            filter,
            CanisterLogFilter::ByTimestampNanos {
                start: 1000,
                end: u64::MAX
            }
        ));
    }

    #[test]
    fn build_filter_until_timestamp_only() {
        let args = make_logs_args(None, Some(2000), None, None);
        let filter = build_filter(&args).unwrap().unwrap();
        assert!(matches!(
            filter,
            CanisterLogFilter::ByTimestampNanos {
                start: 0,
                end: 2000
            }
        ));
    }

    #[test]
    fn build_filter_both_timestamps() {
        let args = make_logs_args(Some(1000), Some(2000), None, None);
        let filter = build_filter(&args).unwrap().unwrap();
        assert!(matches!(
            filter,
            CanisterLogFilter::ByTimestampNanos {
                start: 1000,
                end: 2000
            }
        ));
    }

    #[test]
    fn build_filter_inverted_timestamps_error() {
        let args = make_logs_args(Some(2000), Some(1000), None, None);
        let err = build_filter(&args).unwrap_err().to_string();
        assert!(err.contains("--since timestamp must be less than --until timestamp"));
    }

    #[test]
    fn build_filter_until_index_zero_error() {
        let args = make_logs_args(None, None, None, Some(0));
        let err = build_filter(&args).unwrap_err().to_string();
        assert!(err.contains("--until-index must be greater than 0"));
    }

    #[test]
    fn build_filter_until_timestamp_zero_error() {
        let args = make_logs_args(None, Some(0), None, None);
        let err = build_filter(&args).unwrap_err().to_string();
        assert!(err.contains("--until must be greater than 0"));
    }
}
