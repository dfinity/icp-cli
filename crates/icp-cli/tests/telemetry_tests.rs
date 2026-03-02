//! Integration tests for the telemetry pipeline.
//!
//! Tests are grouped by the step of the control-flow diagram in
//! `docs/telemetry.md` they exercise:
//!
//! 1. **Opt-out** — env var and CI checks (`DO_NOT_TRACK`, `ICP_TELEMETRY_DISABLED`, `CI`)
//! 2. **Record append** — events are written to `events.jsonl` with correct JSON shape
//! 3. **First-run notice** — notice is printed once, then suppressed
//! 4. **Send triggers** — time-based and size-based triggers rotate the event log
//! 5. **No rotation when triggers not met** — events.jsonl kept intact
//! 6. **Batch send** — `__telemetry-send-batch`: payload shape, silent failure, file cleanup
//! 7. **Stale batch cleanup** — old/excess batch files pruned when a send is triggered
//! 8. **Machine-id persistence** — same UUID is reused across invocations
//!
//! Full-pipeline tests run `icp settings telemetry` (a fast, network-free
//! command) with `ICP_HOME` set to a known temp path and all opt-out env vars
//! removed, giving precise control over telemetry state files.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use camino_tempfile::tempdir;
use httptest::{Expectation, Server, matchers::*, responders::*};
use icp::prelude::*; // brings in camino Path / PathBuf
use predicates::str as predstr;
use serde_json::Value;
use time::OffsetDateTime;

mod common;
use common::TestContext;

/// A minimal, syntactically-valid NDJSON telemetry record.
const FAKE_RECORD: &str = r#"{"machine_id":"test-machine","platform":"test","arch":"x86_64","version":"0.0.0","command":"version","arguments":[],"success":true,"duration_ms":42}"#;

/// A timestamp guaranteed to be far in the future (~year 2286).
/// Written to `next-send-time` to prevent the time-based send trigger from
/// firing in tests that don't want to trigger a send.
const FAR_FUTURE_SECS: u64 = 9_999_999_999;

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Initialise the telemetry directory for a full-pipeline test:
/// - Creates the directory.
/// - Creates the `notice-shown` marker so the first-run notice is suppressed
///   (unless the test is specifically about the notice).
/// - Optionally writes `next-send-time`.
fn init_telemetry_dir(dir: &Path, next_send_time: Option<u64>) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("notice-shown"), "").unwrap();
    if let Some(t) = next_send_time {
        std::fs::write(dir.join("next-send-time"), t.to_string()).unwrap();
    }
}

/// Count `batch-*.jsonl` files in the telemetry directory.
fn count_batch_files(telemetry_dir: &Path) -> usize {
    std::fs::read_dir(telemetry_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with("batch-") && n.ends_with(".jsonl")
                })
                .count()
        })
        .unwrap_or(0)
}

/// Build a base `icp` command with telemetry explicitly **enabled**:
/// sets `ICP_HOME` to a known location and strips all opt-out env vars.
///
/// Returns `(icp_home, cmd)` so callers can both manipulate pre-test state
/// and run the command.
macro_rules! icp_with_telemetry {
    ($ctx:expr) => {{
        let icp_home = $ctx.home_path().join("icp-home");
        let mut cmd = $ctx.icp();
        cmd.env("ICP_HOME", icp_home.as_str())
            .env_remove("CI")
            .env_remove("DO_NOT_TRACK")
            .env_remove("ICP_TELEMETRY_DISABLED");
        (icp_home, cmd)
    }};
    ($ctx:expr, allow_upload) => {{
        let icp_home = $ctx.home_path().join("icp-home");
        let mut cmd = $ctx.icp();
        cmd.env("ICP_HOME", icp_home.as_str())
            .env_remove("CI")
            .env_remove("DO_NOT_TRACK")
            .env_remove("ICP_TELEMETRY_DISABLED")
            .env_remove("ICP_CLI_TEST_NO_TELEMETRY_UPLOAD")
            .env(
                "ICP_TELEMETRY_ENDPOINT",
                "https://telemetry.invalid/v1/events",
            );
        (icp_home, cmd)
    }};
}

/// Each of the three opt-out env vars must prevent any telemetry state from
/// being written (the `telemetry/` directory should not be created at all).
#[test]
fn telemetry_disabled_by_do_not_track() {
    let ctx = TestContext::new();
    let icp_home = ctx.home_path().join("icp-home");

    ctx.icp()
        .env("ICP_HOME", icp_home.as_str())
        .env_remove("CI")
        .env_remove("ICP_TELEMETRY_DISABLED")
        .env("DO_NOT_TRACK", "1")
        .args(["settings", "telemetry"])
        .assert()
        .success();

    assert!(
        !icp_home.join("telemetry").exists(),
        "telemetry dir must not be created when DO_NOT_TRACK is set"
    );
}

#[test]
fn telemetry_disabled_by_icp_telemetry_disabled() {
    let ctx = TestContext::new();
    let icp_home = ctx.home_path().join("icp-home");

    ctx.icp()
        .env("ICP_HOME", icp_home.as_str())
        .env_remove("CI")
        .env_remove("DO_NOT_TRACK")
        .env("ICP_TELEMETRY_DISABLED", "1")
        .args(["settings", "telemetry"])
        .assert()
        .success();

    assert!(
        !icp_home.join("telemetry").exists(),
        "telemetry dir must not be created when ICP_TELEMETRY_DISABLED is set"
    );
}

#[test]
fn telemetry_disabled_by_ci() {
    let ctx = TestContext::new();
    let icp_home = ctx.home_path().join("icp-home");

    ctx.icp()
        .env("ICP_HOME", icp_home.as_str())
        .env_remove("DO_NOT_TRACK")
        .env_remove("ICP_TELEMETRY_DISABLED")
        .env("CI", "true")
        .args(["settings", "telemetry"])
        .assert()
        .success();

    assert!(
        !icp_home.join("telemetry").exists(),
        "telemetry dir must not be created when CI is set"
    );
}

/// A command invocation must append a JSON record to `events.jsonl` that
/// contains all required fields with sensible values.
#[test]
fn telemetry_record_appended_to_events_file() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx);
    let telemetry_dir = icp_home.join("telemetry");

    // Prevent time trigger from firing so events.jsonl is not immediately rotated.
    init_telemetry_dir(&telemetry_dir, Some(FAR_FUTURE_SECS));

    cmd.args(["settings", "telemetry"]).assert().success();

    let events_path = telemetry_dir.join("events.jsonl");
    assert!(events_path.exists(), "events.jsonl must be created");

    let contents = std::fs::read_to_string(&events_path).unwrap();
    let first_line = contents
        .lines()
        .next()
        .expect("events.jsonl must contain at least one record");
    let record: Value = serde_json::from_str(first_line).expect("record must be valid JSON");

    // Required fields
    assert!(
        record["machine_id"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "machine_id must be a non-empty string"
    );
    assert!(
        !record["platform"].as_str().unwrap_or("").is_empty(),
        "platform must be present"
    );
    assert!(
        !record["arch"].as_str().unwrap_or("").is_empty(),
        "arch must be present"
    );
    assert!(
        !record["version"].as_str().unwrap_or("").is_empty(),
        "version must be present"
    );
    // date must be today's UTC date in YYYY-MM-DD format
    let today = OffsetDateTime::now_utc().date().to_string();
    assert_eq!(
        record["date"].as_str().unwrap_or(""),
        today,
        "date must be today's UTC date in YYYY-MM-DD format"
    );
    // command is "settings telemetry" (subcommand path joined with spaces)
    assert_eq!(record["command"], "settings telemetry");
    assert!(
        record["duration_ms"].is_number(),
        "duration_ms must be a number"
    );
    // The command succeeds → success = true
    assert_eq!(record["success"], true);
}

/// On the very first run (no `notice-shown` marker) the notice must be printed
/// to stderr.  On subsequent runs it must be suppressed.
#[test]
fn telemetry_first_run_notice_printed_once() {
    let ctx = TestContext::new();
    let icp_home = ctx.home_path().join("icp-home");
    let telemetry_dir = icp_home.join("telemetry");

    // No init_telemetry_dir — deliberately start without a notice-shown marker.
    // Prevent the time trigger to avoid rotation noise, but still let the
    // notice logic run by NOT creating notice-shown.
    std::fs::create_dir_all(&telemetry_dir).unwrap();
    std::fs::write(
        telemetry_dir.join("next-send-time"),
        FAR_FUTURE_SECS.to_string(),
    )
    .unwrap();

    // First run: notice must appear in stderr.
    ctx.icp()
        .env("ICP_HOME", icp_home.as_str())
        .env_remove("CI")
        .env_remove("DO_NOT_TRACK")
        .env_remove("ICP_TELEMETRY_DISABLED")
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stderr(predstr::contains("anonymous usage data"));

    // Second run: notice must NOT appear again.
    ctx.icp()
        .env("ICP_HOME", icp_home.as_str())
        .env_remove("CI")
        .env_remove("DO_NOT_TRACK")
        .env_remove("ICP_TELEMETRY_DISABLED")
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stderr(predstr::is_empty());
}

/// If the `notice-shown` marker already exists the notice must never be shown.
#[test]
fn telemetry_notice_suppressed_when_marker_exists() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx);
    let telemetry_dir = icp_home.join("telemetry");

    // init_telemetry_dir creates the notice-shown marker.
    init_telemetry_dir(&telemetry_dir, Some(FAR_FUTURE_SECS));

    cmd.args(["settings", "telemetry"])
        .assert()
        .success()
        .stderr(predstr::is_empty());
}

/// When `next-send-time` is in the past the event log must be rotated to a
/// `batch-*.jsonl` file and `events.jsonl` must no longer exist.
#[test]
fn telemetry_time_trigger_rotates_events() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx, allow_upload);
    let telemetry_dir = icp_home.join("telemetry");

    // Set next-send-time to Unix epoch (far in the past).
    init_telemetry_dir(&telemetry_dir, Some(0));

    cmd.args(["settings", "telemetry"]).assert().success();

    // events.jsonl must have been rotated away.
    assert!(
        !telemetry_dir.join("events.jsonl").exists(),
        "events.jsonl must be rotated when next-send-time is in the past"
    );
    assert!(
        count_batch_files(&telemetry_dir) >= 1,
        "at least one batch-*.jsonl must exist after rotation"
    );
}

/// When `events.jsonl` exceeds 256 KB the event log must be rotated even if
/// the time-based trigger has not fired.
#[test]
fn telemetry_size_trigger_rotates_events() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx, allow_upload);
    let telemetry_dir = icp_home.join("telemetry");

    // next-send-time far in the future → only the size trigger can fire.
    init_telemetry_dir(&telemetry_dir, Some(FAR_FUTURE_SECS));

    // Write > 256 KB to events.jsonl.
    let big_record = format!("{}\n", FAKE_RECORD).repeat(3000); // ~300 KB
    std::fs::write(telemetry_dir.join("events.jsonl"), &big_record).unwrap();

    cmd.args(["settings", "telemetry"]).assert().success();

    assert!(
        count_batch_files(&telemetry_dir) >= 1,
        "at least one batch-*.jsonl must exist after a size-triggered rotation"
    );
}

/// When neither threshold is met (time in future, file small) the event log
/// must not be rotated.
#[test]
fn telemetry_no_rotation_when_send_not_due() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx);
    let telemetry_dir = icp_home.join("telemetry");

    init_telemetry_dir(&telemetry_dir, Some(FAR_FUTURE_SECS));

    cmd.args(["settings", "telemetry"]).assert().success();

    assert!(
        telemetry_dir.join("events.jsonl").exists(),
        "events.jsonl must be kept when no send threshold is met"
    );
    assert_eq!(
        count_batch_files(&telemetry_dir),
        0,
        "no batch files must exist when no send is triggered"
    );
}

/// A failed batch send (default `.invalid` endpoint) must exit 0, produce no
/// output, and leave the batch file in place for retry.
#[test]
fn telemetry_failed_send_is_silent() {
    let ctx = TestContext::new();
    let dir = tempdir().expect("temp dir");

    let batch_path = dir
        .path()
        .join("batch-1700000000-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl");
    std::fs::write(&batch_path, FAKE_RECORD).expect("write batch file");

    ctx.icp()
        .args(["__telemetry-send-batch", batch_path.as_str()])
        .timeout(Duration::from_secs(15))
        .assert()
        .success()
        .stdout(predstr::is_empty())
        .stderr(predstr::is_empty());

    assert!(
        batch_path.exists(),
        "batch file must be retained after a failed send"
    );
}

/// A successful batch send must POST the payload to the endpoint, produce no
/// output, and delete the batch file on HTTP 200.
#[test]
fn telemetry_send_batch_delivers_data() {
    let ctx = TestContext::new();
    let dir = tempdir().expect("temp dir");

    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::method("POST"),
            request::path("/v1/events"),
            request::headers(contains(("content-type", "application/x-ndjson"))),
            // add_batch_metadata injects "batch" and "sequence" into each record.
            request::body(matches("\"batch\"")),
            request::body(matches("\"sequence\"")),
            // Original fields must be preserved.
            request::body(matches("\"machine_id\":\"test-machine\"")),
            request::body(matches("\"command\":\"version\"")),
        ])
        .times(1)
        .respond_with(status_code(200)),
    );
    let endpoint = format!("http://{}/v1/events", server.addr());

    let batch_path = dir
        .path()
        .join("batch-1700000001-bbbbbbbb-cccc-dddd-eeee-ffffffffffff.jsonl");
    std::fs::write(&batch_path, FAKE_RECORD).expect("write batch file");

    ctx.icp()
        .args(["__telemetry-send-batch", batch_path.as_str()])
        .env("ICP_TELEMETRY_ENDPOINT", &endpoint)
        .assert()
        .success()
        .stdout(predstr::is_empty())
        .stderr(predstr::is_empty());

    // Absence of the batch file proves the server returned HTTP 200.
    assert!(
        !batch_path.exists(),
        "batch file must be deleted after a successful send"
    );

    // Server drop verifies the POST expectation was met exactly once.
}

/// Batch files older than 14 days must be deleted when a new send is triggered.
#[test]
fn telemetry_stale_batches_deleted_on_trigger() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx, allow_upload);
    let telemetry_dir = icp_home.join("telemetry");

    // Time trigger will fire.
    init_telemetry_dir(&telemetry_dir, Some(0));

    // Plant three batch files whose embedded timestamps are 15 days in the past.
    let stale_ts = unix_now().saturating_sub(15 * 24 * 3600);
    for i in 0..3u8 {
        let name = format!("batch-{stale_ts}-stale-stale-stale-stale{i:012}.jsonl");
        std::fs::write(telemetry_dir.join(&name), FAKE_RECORD).unwrap();
    }

    assert_eq!(
        count_batch_files(&telemetry_dir),
        3,
        "pre-condition: 3 stale batches"
    );

    cmd.args(["settings", "telemetry"]).assert().success();

    // All three stale files must be gone; only the freshly-rotated batch remains.
    let remaining = count_batch_files(&telemetry_dir);
    assert!(
        remaining <= 1,
        "stale batches must be deleted; found {remaining} batch file(s) after cleanup"
    );
}

/// When more than 10 batch files exist, the oldest ones must be pruned down
/// to 10 when the next send is triggered.
#[test]
fn telemetry_excess_batches_pruned_on_trigger() {
    let ctx = TestContext::new();
    let (icp_home, mut cmd) = icp_with_telemetry!(ctx, allow_upload);
    let telemetry_dir = icp_home.join("telemetry");

    // Time trigger will fire.
    init_telemetry_dir(&telemetry_dir, Some(0));

    // Plant 11 batch files with slightly staggered timestamps (all recent,
    // so age-based cleanup does not apply).
    let base_ts = unix_now().saturating_sub(3600); // 1 hour ago
    for i in 0..11u64 {
        let ts = base_ts + i;
        let name = format!("batch-{ts}-excess-exce-ss-e-xcesse{i:08}.jsonl");
        std::fs::write(telemetry_dir.join(&name), FAKE_RECORD).unwrap();
    }

    assert_eq!(
        count_batch_files(&telemetry_dir),
        11,
        "pre-condition: 11 batch files"
    );

    // Running the command triggers a send, which rotates events.jsonl
    // (→ 12 files) and then prunes back to 10.
    cmd.args(["settings", "telemetry"]).assert().success();

    let remaining = count_batch_files(&telemetry_dir);
    assert!(
        remaining <= 10,
        "batch count must be pruned to ≤10; found {remaining}"
    );
}

/// The same `machine_id` UUID must appear in all records produced by
/// consecutive command invocations.
#[test]
fn telemetry_machine_id_persists_across_invocations() {
    let ctx = TestContext::new();
    let icp_home = ctx.home_path().join("icp-home");
    let telemetry_dir = icp_home.join("telemetry");

    // Keep next-send-time in the future so events.jsonl is never rotated
    // and both records land in the same file.
    init_telemetry_dir(&telemetry_dir, Some(FAR_FUTURE_SECS));

    for _ in 0..2 {
        ctx.icp()
            .env("ICP_HOME", icp_home.as_str())
            .env_remove("CI")
            .env_remove("DO_NOT_TRACK")
            .env_remove("ICP_TELEMETRY_DISABLED")
            .args(["settings", "telemetry"])
            .assert()
            .success();
    }

    let contents = std::fs::read_to_string(telemetry_dir.join("events.jsonl")).unwrap();
    let ids: Vec<&str> = contents
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            v["machine_id"]
                .as_str()
                .map(str::to_owned)
                .map(|s| Box::leak(s.into_boxed_str()) as &str)
        })
        .collect();

    assert_eq!(ids.len(), 2, "expected 2 records");
    assert_eq!(
        ids[0], ids[1],
        "machine_id must be identical across invocations: got {:?} and {:?}",
        ids[0], ids[1]
    );
}
