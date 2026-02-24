//! Telemetry collection and submission for icp-cli.
//!
//! Collects anonymous usage data (command name, arguments, duration, outcome)
//! and periodically ships it in a detached background process. All I/O errors
//! are silently ignored so telemetry never affects CLI behaviour.

use std::{
    io::Write as _,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use clap::parser::ValueSource;
use icp::prelude::*;
use icp::settings::Settings;
use icp::telemetry_data::{IdentityStorageType, NetworkType, TelemetryData};
use rand::Rng as _;
use serde::{Deserialize, Serialize};

use crate::version::icp_cli_version_str;

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

/// Telemetry ingestion endpoint.
///
/// Deliberately set to a non-resolvable placeholder until the real endpoint is
/// confirmed.  The `.invalid` TLD is reserved by RFC 2606 and will never
/// resolve, so any send attempt fails silently without reaching a third-party
/// server.
///
/// Override at runtime with the `ICP_TELEMETRY_ENDPOINT` environment variable
/// (intended for integration tests only).
const TELEMETRY_ENDPOINT: &str = "https://telemetry.invalid/v1/events";

// ---------------------------------------------------------------------------
// Argument types
// ---------------------------------------------------------------------------

/// How an argument was supplied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ArgumentSource {
    CommandLine,
    Environment,
}

/// A single CLI argument recorded for telemetry.
///
/// `value` is only populated when the argument has a constrained set of
/// `possible_values` in its clap definition and the actual value matches one
/// of them. Free-form values (paths, principals, etc.) are always `None` to
/// avoid leaking sensitive data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Argument {
    pub name: String,
    pub value: Option<String>,
    pub source: ArgumentSource,
}

// ---------------------------------------------------------------------------
// Record type
// ---------------------------------------------------------------------------

/// A single telemetry event appended to `events.jsonl`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TelemetryRecord {
    // --- Metadata that is constant across all events on the same machine
    pub machine_id: String,
    pub platform: String,
    pub arch: &'static str,

    pub version: String,

    // --- About command itself
    pub command: String,
    pub arguments: Vec<Argument>,

    // --- Global settings that may affect command behaviour
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autocontainerize: Option<bool>,

    // --- Basic outcome metrics
    pub success: bool,
    pub duration_ms: u64,

    // --- From TelemetryData (set during the execution of the command)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_type: Option<IdentityStorageType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_type: Option<NetworkType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_canisters: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipes: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Session — wraps a single command invocation
// ---------------------------------------------------------------------------

/// Tracks the timing and metadata of one CLI invocation.
pub(crate) struct TelemetrySession {
    start: Instant,
    telemetry_dir: PathBuf,
    command: String,
    arguments: Vec<Argument>,
    autocontainerize: Option<bool>,
}

impl TelemetrySession {
    /// Begin a session.
    pub(crate) fn begin(
        telemetry_dir: PathBuf,
        command: String,
        arguments: Vec<Argument>,
        autocontainerize: Option<bool>,
    ) -> Self {
        Self {
            start: Instant::now(),
            telemetry_dir,
            command,
            arguments,
            autocontainerize,
        }
    }

    /// Finish the session, record the event, and trigger a send if needed.
    pub(crate) fn finish(self, success: bool, telemetry_data: &TelemetryData) {
        let machine_id = get_or_create_machine_id(&self.telemetry_dir);
        let duration_ms = self.start.elapsed().as_millis() as u64;

        let record = TelemetryRecord {
            machine_id,
            platform: if cfg!(target_os = "linux") && std::env::var_os("WSL_DISTRO_NAME").is_some()
            {
                "wsl".to_string()
            } else {
                std::env::consts::OS.to_string()
            },
            arch: std::env::consts::ARCH,
            version: icp_cli_version_str().to_string(),
            command: self.command,
            arguments: self.arguments,
            autocontainerize: self.autocontainerize,
            success,
            duration_ms,
            identity_type: telemetry_data.identity_type(),
            network_type: telemetry_data.network_type(),
            num_canisters: telemetry_data.num_canisters(),
            recipes: telemetry_data.recipes(),
        };

        append_record(&self.telemetry_dir, &record);
        maybe_send(&self.telemetry_dir);
    }
}

// ---------------------------------------------------------------------------
// High-level setup — called from main.rs
// ---------------------------------------------------------------------------

/// Initialise a telemetry session unless telemetry is disabled.
pub(crate) async fn setup(
    ctx: &icp::context::Context,
    raw_args: &[String],
    clap_command: &clap::Command,
) -> Option<TelemetrySession> {
    if is_disabled_by_env() {
        return None;
    }

    let telemetry_dir = ctx.dirs.telemetry_data();

    // Load settings to check the user preference (best-effort; default to enabled).
    // Also capture `autocontainerize` while settings are in hand.
    let (enabled, autocontainerize) = async {
        let dirs = ctx.dirs.settings().ok()?;
        let settings = dirs
            .with_read(async |dirs| Settings::load_from(dirs))
            .await
            .ok()?
            .ok()?;
        Some((settings.telemetry_enabled, settings.autocontainerize))
    }
    .await
    .map(|(e, a)| (e, Some(a)))
    .unwrap_or((true, None));

    if !enabled {
        return None;
    }

    show_notice_if_needed(&telemetry_dir);

    // Re-parse raw args into ArgMatches to derive command name and arguments
    // in one pass. This never fails in practice since Cli::parse() already
    // succeeded.
    let (cmd_name, arguments) = clap_command
        .clone()
        .try_get_matches_from(raw_args)
        .map(|m| collect_command_and_arguments(&m, clap_command))
        .unwrap_or_default();

    Some(TelemetrySession::begin(
        telemetry_dir,
        cmd_name,
        arguments,
        autocontainerize,
    ))
}

// ---------------------------------------------------------------------------
// Opt-out checks
// ---------------------------------------------------------------------------

/// Returns `true` if any of the standard opt-out env vars are set.
fn is_disabled_by_env() -> bool {
    std::env::var_os("DO_NOT_TRACK").is_some()
        || std::env::var_os("ICP_TELEMETRY_DISABLED").is_some()
        || std::env::var_os("CI").is_some()
}

// ---------------------------------------------------------------------------
// First-run notice
// ---------------------------------------------------------------------------

/// Prints the first-run notice and creates the marker file if it has not been
/// shown before.  Errors are silently swallowed.
fn show_notice_if_needed(telemetry_dir: &Path) {
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
    if let Ok(meta) = std::fs::metadata(telemetry_dir.join(EVENTS_FILE))
        && meta.len() >= MAX_EVENTS_SIZE_BYTES
    {
        return true;
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
fn maybe_send(telemetry_dir: &Path) {
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

    // Atomically rotate events.jsonl out of the write path.
    // The filename encodes both a timestamp (for age-based cleanup) and a
    // batch UUID (used to tag records on send for server-side deduplication).
    let batch_id = uuid::Uuid::new_v4();
    let batch_name = format!("batch-{}-{batch_id}.jsonl", unix_now());
    let batch_path = telemetry_dir.join(&batch_name);
    if std::fs::rename(&events_path, &batch_path).is_err() {
        return;
    }

    cleanup_stale_batches(telemetry_dir);
    spawn_send_batch(&batch_path);
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
            if name.starts_with("batch-") && name.ends_with(".jsonl") {
                PathBuf::try_from(e.path()).ok()
            } else {
                None
            }
        })
        .collect();

    // Delete batches that are too old
    batches.retain(|p| {
        // Extract timestamp from "batch-<timestamp>-<uuid>.jsonl"
        let ts: Option<u64> = p
            .file_name()
            .and_then(|n| n.strip_prefix("batch-"))
            .and_then(|n| n.split('-').next())
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
fn spawn_send_batch(batch_path: &Path) {
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

    // Extract the batch UUID from the filename ("batch-<ts>-<uuid>.jsonl").
    let batch_id = batch_path
        .file_stem()
        .and_then(|s| s.splitn(3, '-').nth(2))
        .unwrap_or("unknown");

    let Ok(payload) = add_batch_metadata(&contents, batch_id) else {
        return;
    };

    let endpoint =
        std::env::var("ICP_TELEMETRY_ENDPOINT").unwrap_or_else(|_| TELEMETRY_ENDPOINT.to_string());

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let result = client
        .post(&endpoint)
        .header("Content-Type", "application/x-ndjson")
        .body(payload)
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

/// Inject a shared `batch` UUID and per-line `sequence` number into each
/// JSON record. This allows the server to deduplicate retried sends and
/// reconstruct event ordering within a batch.
fn add_batch_metadata(contents: &str, batch_id: &str) -> Result<String, serde_json::Error> {
    let mut lines = Vec::new();

    for (seq, line) in contents.lines().enumerate() {
        let mut json: serde_json::Value = serde_json::from_str(line)?;
        json["batch"] = serde_json::Value::String(batch_id.to_string());
        json["sequence"] = serde_json::Value::Number(serde_json::Number::from(seq));
        lines.push(serde_json::to_string(&json)?);
    }

    Ok(lines.join("\n"))
}

fn write_next_send_time(telemetry_dir: &Path) {
    let is_prerelease = env!("CARGO_PKG_VERSION").contains('-');
    let next = unix_now() + random_send_interval(is_prerelease);
    let _ = std::fs::write(telemetry_dir.join(NEXT_SEND_TIME_FILE), next.to_string());
}

// ---------------------------------------------------------------------------
// Argument extraction from clap
// ---------------------------------------------------------------------------

/// Walk `ArgMatches` / `Command` down to the leaf subcommand, collecting
/// subcommand names along the way and returning the deepest matches and
/// the corresponding command definition.
fn get_deepest_subcommand<'a>(
    matches: &'a clap::ArgMatches,
    command: &'a clap::Command,
) -> (Vec<&'a str>, &'a clap::ArgMatches, &'a clap::Command) {
    let mut command_names = Vec::new();
    let mut deepest_matches = matches;
    let mut deepest_command = command;

    while let Some((name, sub_matches)) = deepest_matches.subcommand() {
        if let Some(sub_cmd) = deepest_command
            .get_subcommands()
            .find(|c| c.get_name() == name)
        {
            command_names.push(name);
            deepest_matches = sub_matches;
            deepest_command = sub_cmd;
        } else {
            break;
        }
    }

    (command_names, deepest_matches, deepest_command)
}

/// Extract sanitized arguments from clap's parsed state.
///
/// For each argument that was explicitly provided (not a default):
/// - Records the argument name and how it was supplied (CLI vs env var).
/// - Includes the value **only** when the argument has a constrained set of
///   `possible_values` and the actual value matches one of them, preventing
///   free-form user input (paths, principals, etc.) from leaking into telemetry.
fn collect_command_and_arguments(
    arg_matches: &clap::ArgMatches,
    command: &clap::Command,
) -> (String, Vec<Argument>) {
    let (command_names, deepest_matches, deepest_command) =
        get_deepest_subcommand(arg_matches, command);
    let command_name = command_names.join(" ");

    let mut arguments = Vec::new();

    for id in deepest_matches.ids() {
        let id_str = id.as_str();

        let source = match deepest_matches.value_source(id_str) {
            Some(ValueSource::CommandLine) => ArgumentSource::CommandLine,
            Some(ValueSource::EnvVariable) => ArgumentSource::Environment,
            _ => continue,
        };

        let possible_values = deepest_command
            .get_arguments()
            .find(|arg| arg.get_id() == id_str)
            .map(|arg| arg.get_possible_values());

        let sanitized_value = match (
            possible_values,
            deepest_matches.try_get_one::<String>(id_str),
        ) {
            (Some(possible_values), Ok(Some(s)))
                if possible_values.iter().any(|pv| pv.matches(s, true)) =>
            {
                Some(s.clone())
            }
            _ => None,
        };

        arguments.push(Argument {
            name: id_str.to_string(),
            value: sanitized_value,
            source,
        });
    }

    (command_name, arguments)
}
