//! Telemetry collection and submission for icp-cli.
//!
//! Collects anonymous usage data (command name, flags, duration, outcome) and
//! periodically ships it in a detached background process. All I/O errors are
//! silently ignored so telemetry never affects CLI behaviour.

use std::{
    io::Write as _,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use icp::prelude::*;
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const EVENTS_FILE: &str = "events.jsonl";
const MACHINE_ID_FILE: &str = "machine-id";
const NOTICE_SHOWN_FILE: &str = "notice-shown";
const NEXT_SEND_TIME_FILE: &str = "next-send-time";

/// Maximum size of events.jsonl before a size-triggered send.
const MAX_EVENTS_SIZE_BYTES: u64 = 256 * 1024;

/// Maximum age of a pending batch file before it is discarded (in seconds).
const MAX_BATCH_AGE_SECS: u64 = 14 * 24 * 3600;

/// Maximum number of pending batch files before old ones are discarded.
const MAX_BATCH_COUNT: usize = 10;

/// How long to guard the send slot while a background send is in flight (seconds).
const SEND_GUARD_SECS: u64 = 30 * 60;

/// Telemetry ingestion endpoint. Replace with the real URL before GA.
const TELEMETRY_ENDPOINT: &str = "https://telemetry.icp-cli.dev/v1/events";

// ---------------------------------------------------------------------------
// Record type
// ---------------------------------------------------------------------------

/// A single telemetry event appended to `events.jsonl`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TelemetryRecord {
    pub version: String,
    pub os: &'static str,
    pub arch: &'static str,
    pub command: String,
    pub flags: Vec<String>,
    pub success: bool,
    pub duration_ms: u64,
    pub machine_id: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Session — wraps a single command invocation
// ---------------------------------------------------------------------------

/// Tracks the timing and metadata of one CLI invocation.
pub(crate) struct TelemetrySession {
    start: Instant,
    telemetry_dir: PathBuf,
    command: String,
    flags: Vec<String>,
    version: String,
}

impl TelemetrySession {
    /// Begin a session.
    pub(crate) fn begin(
        telemetry_dir: PathBuf,
        command: String,
        flags: Vec<String>,
        version: String,
    ) -> Self {
        Self {
            start: Instant::now(),
            telemetry_dir,
            command,
            flags,
            version,
        }
    }

    /// Finish the session, record the event, and trigger a send if needed.
    pub(crate) fn finish(self, success: bool) {
        let duration_ms = self.start.elapsed().as_millis() as u64;
        let machine_id = get_or_create_machine_id(&self.telemetry_dir);

        let timestamp = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let record = TelemetryRecord {
            version: self.version.clone(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            command: self.command,
            flags: self.flags,
            success,
            duration_ms,
            machine_id,
            timestamp,
        };

        append_record(&self.telemetry_dir, &record);
        maybe_send(&self.telemetry_dir, &self.version);
    }
}

// ---------------------------------------------------------------------------
// Opt-out checks
// ---------------------------------------------------------------------------

/// Returns `true` if any of the standard opt-out env vars are set.
pub(crate) fn is_disabled_by_env() -> bool {
    std::env::var_os("DO_NOT_TRACK").is_some()
        || std::env::var_os("ICP_TELEMETRY_DISABLED").is_some()
        || std::env::var_os("CI").is_some()
}

// ---------------------------------------------------------------------------
// First-run notice
// ---------------------------------------------------------------------------

/// Prints the first-run notice and creates the marker file if it has not been
/// shown before.  Errors are silently swallowed.
pub(crate) fn show_notice_if_needed(telemetry_dir: &Path) {
    let marker = telemetry_dir.join(NOTICE_SHOWN_FILE);
    if marker.exists() {
        return;
    }
    eprintln!(
        "icp collects anonymous usage data to improve the tool.\n\
         Run `icp settings telemetry false` or set DO_NOT_TRACK=1 to opt out.\n\
         Learn more: https://docs.icp-cli.dev/telemetry"
    );
    let _ = std::fs::create_dir_all(telemetry_dir);
    let _ = std::fs::write(&marker, "");
}

// ---------------------------------------------------------------------------
// Machine ID
// ---------------------------------------------------------------------------

fn get_or_create_machine_id(telemetry_dir: &Path) -> String {
    let path = telemetry_dir.join(MACHINE_ID_FILE);
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(telemetry_dir);
    let _ = std::fs::write(&path, &id);
    id
}

// ---------------------------------------------------------------------------
// Event log
// ---------------------------------------------------------------------------

fn append_record(telemetry_dir: &Path, record: &TelemetryRecord) {
    let Ok(line) = serde_json::to_string(record) else {
        return;
    };
    let events_path = telemetry_dir.join(EVENTS_FILE);
    let _ = std::fs::create_dir_all(telemetry_dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)
    {
        let _ = writeln!(f, "{line}");
    }
}

// ---------------------------------------------------------------------------
// Send triggering
// ---------------------------------------------------------------------------

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Initialise `next-send-time` to a random interval in the future on first run.
fn init_next_send_time(telemetry_dir: &Path, is_prerelease: bool) {
    let next = unix_now() + random_send_interval(is_prerelease);
    let _ = std::fs::create_dir_all(telemetry_dir);
    let _ = std::fs::write(telemetry_dir.join(NEXT_SEND_TIME_FILE), next.to_string());
}

/// Returns a randomised send interval in seconds.
/// Stable: 2–4 days.  Pre-release: 0.75–1.25 days.
fn random_send_interval(is_prerelease: bool) -> u64 {
    let mut rng = rand::rng();
    if is_prerelease {
        let base = 18 * 3600u64; // 0.75 days
        let jitter = 12 * 3600u64; // ±0.25 days
        base + rng.random_range(0..jitter)
    } else {
        let base = 2 * 24 * 3600u64; // 2 days
        let jitter = 2 * 24 * 3600u64; // up to +2 days
        base + rng.random_range(0..jitter)
    }
}

fn should_send(telemetry_dir: &Path) -> bool {
    let is_prerelease = env!("CARGO_PKG_VERSION").contains('-');

    // Size-based trigger
    if let Ok(meta) = std::fs::metadata(telemetry_dir.join(EVENTS_FILE)) {
        if meta.len() >= MAX_EVENTS_SIZE_BYTES {
            return true;
        }
    }

    // Time-based trigger
    let next_send_path = telemetry_dir.join(NEXT_SEND_TIME_FILE);
    match std::fs::read_to_string(&next_send_path) {
        Ok(content) => {
            if let Ok(next_send) = content.trim().parse::<u64>() {
                return unix_now() >= next_send;
            }
            // Unreadable content — reinitialise
            init_next_send_time(telemetry_dir, is_prerelease);
        }
        Err(_) => {
            // File absent — first run: initialise and don't send yet
            init_next_send_time(telemetry_dir, is_prerelease);
        }
    }

    false
}

/// Trigger a batch send if either threshold is met.
fn maybe_send(telemetry_dir: &Path, version: &str) {
    if !should_send(telemetry_dir) {
        return;
    }

    let events_path = telemetry_dir.join(EVENTS_FILE);
    if !events_path.exists() {
        return;
    }

    // Set the concurrent-send guard: next-send-time = now + 30 min
    let guard_until = unix_now() + SEND_GUARD_SECS;
    let _ = std::fs::write(
        telemetry_dir.join(NEXT_SEND_TIME_FILE),
        guard_until.to_string(),
    );

    // Atomically rotate events.jsonl out of the write path
    let batch_name = format!("events-sending-{}.jsonl", unix_now());
    let batch_path = telemetry_dir.join(&batch_name);
    if std::fs::rename(&events_path, &batch_path).is_err() {
        return;
    }

    cleanup_stale_batches(telemetry_dir);
    spawn_send_batch(&batch_path, version);
}

// ---------------------------------------------------------------------------
// Stale batch cleanup
// ---------------------------------------------------------------------------

fn cleanup_stale_batches(telemetry_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(telemetry_dir) else {
        return;
    };

    let cutoff = unix_now().saturating_sub(MAX_BATCH_AGE_SECS);

    let mut batches: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("events-sending-") && name.ends_with(".jsonl") {
                PathBuf::try_from(e.path()).ok()
            } else {
                None
            }
        })
        .collect();

    // Delete batches that are too old
    batches.retain(|p| {
        let ts: Option<u64> = p
            .file_name()
            .and_then(|n| n.strip_prefix("events-sending-"))
            .and_then(|n| n.strip_suffix(".jsonl"))
            .and_then(|n| n.parse().ok());

        if ts.map(|t| t < cutoff).unwrap_or(false) {
            let _ = std::fs::remove_file(p);
            return false;
        }
        true
    });

    // Delete the oldest ones if there are too many
    if batches.len() > MAX_BATCH_COUNT {
        batches.sort();
        for p in batches.iter().take(batches.len() - MAX_BATCH_COUNT) {
            let _ = std::fs::remove_file(p);
        }
    }
}

// ---------------------------------------------------------------------------
// Background send process
// ---------------------------------------------------------------------------

/// Spawn a detached child process that sends the batch file.
fn spawn_send_batch(batch_path: &Path, _version: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__telemetry-send-batch")
        .arg(batch_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Detach the child from the parent's process group / console so it
    // survives if the parent is killed or the terminal is closed.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let _ = cmd.spawn();
}

/// Entry point for the hidden `__telemetry-send-batch <path>` mode.
///
/// POSTs the batch file to the ingestion endpoint, updates `next-send-time` on
/// success, and exits.  All errors are silent.
pub(crate) async fn handle_send_batch(batch_path_str: &str) {
    let batch_path = Path::new(batch_path_str);

    let Ok(contents) = std::fs::read_to_string(batch_path) else {
        return;
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let result = client
        .post(TELEMETRY_ENDPOINT)
        .header("Content-Type", "application/x-ndjson")
        .body(contents)
        .send()
        .await;

    if result.is_ok_and(|r| r.status().is_success()) {
        let _ = std::fs::remove_file(batch_path);
        if let Some(dir) = batch_path.parent() {
            write_next_send_time(dir);
        }
    }
    // On failure the batch file remains for retry on the next trigger.
}

fn write_next_send_time(telemetry_dir: &Path) {
    let is_prerelease = env!("CARGO_PKG_VERSION").contains('-');
    let next = unix_now() + random_send_interval(is_prerelease);
    let _ = std::fs::write(telemetry_dir.join(NEXT_SEND_TIME_FILE), next.to_string());
}

// ---------------------------------------------------------------------------
// Flag extraction from raw args
// ---------------------------------------------------------------------------

/// Collect flag names (e.g. `--network`, `-v`) from a raw argument list,
/// discarding values.  Subcommand names and positional args are ignored.
pub(crate) fn collect_flags(args: &[String]) -> Vec<String> {
    args.iter()
        .filter_map(|arg| {
            if arg.starts_with('-') {
                // Strip any `=value` suffix
                let flag = arg.splitn(2, '=').next().unwrap_or(arg);
                Some(flag.to_string())
            } else {
                None
            }
        })
        .collect()
}
