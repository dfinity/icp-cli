use clap::{Parser, Subcommand};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) mod allowance;
pub(crate) mod approve;
pub(crate) mod balance;
pub(crate) mod transfer;

/// Format an ICRC-2 expiry (nanoseconds since the Unix epoch) as a human-readable
/// RFC 3339 timestamp, falling back to the raw nanoseconds if it cannot be rendered.
pub(crate) fn format_expiry(expires_at_nanos: u64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(expires_at_nanos as i128)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| expires_at_nanos.to_string())
}

/// Perform token transactions
#[derive(Debug, Parser)]
pub(crate) struct Command {
    /// The token or ledger canister id to execute the operation on, defaults to `icp`
    #[arg(default_value = "icp", value_name = "TOKEN|LEDGER_ID")]
    pub(crate) token_name_or_ledger_id: String,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Balance(balance::BalanceArgs),
    Transfer(transfer::TransferArgs),
    Approve(approve::ApproveArgs),
    Allowance(allowance::AllowanceArgs),
}
