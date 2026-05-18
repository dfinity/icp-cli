//! Miscellaneous utilities that don't belong to specific commands.

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
