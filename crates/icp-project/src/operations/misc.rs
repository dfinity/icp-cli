//! Miscellaneous utilities that don't belong to specific commands.

use time::{OffsetDateTime, macros::format_description};

use crate::calls::CanisterCalls;

/// A canister's custom-section metadata as text, or `None` when it has no such
/// section (or could not be asked).
///
/// Callers use this to detect a capability — a Motoko EOP canister, a canister
/// that publishes its Candid — so "absent" and "could not tell" lead to the
/// same conservative answer and are not worth distinguishing.
pub async fn fetch_canister_metadata(
    calls: &dyn CanisterCalls,
    canister_id: candid::Principal,
    metadata: &str,
) -> Option<String> {
    let section = calls.metadata_section(canister_id, metadata).await.ok()??;
    Some(String::from_utf8_lossy(&section).into())
}

/// Format a nanosecond timestamp as a human-readable UTC datetime string.
pub fn format_timestamp(nanos: u64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp_nanos(nanos as i128) else {
        return nanos.to_string();
    };
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    datetime
        .format(&format)
        .unwrap_or_else(|_| nanos.to_string())
}
