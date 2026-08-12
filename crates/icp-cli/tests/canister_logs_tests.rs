#[cfg(unix)]
use {
    crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext},
    icp::fs::write_string,
    indoc::formatdoc,
    predicates::prelude::PredicateBooleanExt,
    predicates::str::contains,
    std::io::{BufRead as _, BufReader},
    std::process::Stdio,
    std::time::Duration,
};

mod common;

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_single_fetch() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    // Copy the logger canister assets
    ctx.copy_asset_dir("canister_logs", &project_dir);

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "logger", "--environment", "random-environment"])
        .assert()
        .success();

    // Call log() to create some logs
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "logger",
            "log",
            "(\"Test message 1\")",
        ])
        .assert()
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "logger",
            "log",
            "(\"Test message 2\")",
        ])
        .assert()
        .success();

    // Fetch logs: the default emits the human-readable "[idx. timestamp]: content" lines,
    // not JSON. (Regression: this orientation was once swapped with `--json`.)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("]: Test message 1"))
        .stdout(contains("]: Test message 2"))
        .stdout(contains("log_records").not());

    // With --json: parseable JSON in the JsonListRecord shape, terminated by a newline.
    let json_stdout = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--json",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json_stdout = String::from_utf8(json_stdout).expect("stdout is not valid UTF-8");
    assert!(
        json_stdout.ends_with('\n'),
        "--json output should end with a newline, got {json_stdout:?}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(json_stdout.trim()).expect("--json output should be valid JSON");
    let records = parsed["log_records"]
        .as_array()
        .expect("--json output should contain a log_records array");
    for message in ["Test message 1", "Test message 2"] {
        let record = records
            .iter()
            .find(|r| r["content"].as_str() == Some(message))
            .unwrap_or_else(|| panic!("--json output should contain {message:?}"));
        assert!(record["timestamp"].is_u64());
        assert!(record["index"].is_u64());
    }
}

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_follow_mode() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    // Copy the logger canister assets
    ctx.copy_asset_dir("canister_logs", &project_dir);

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "logger", "--environment", "random-environment"])
        .assert()
        .success();

    // Trigger repeated logging (will log 5 times over 5 seconds)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "logger",
            "log_repeated",
            "(\"Repeated\")",
        ])
        .assert()
        .success();

    // Start following logs with a timeout of 7 seconds (enough to see several logs)
    // The logs are not present yet, so if e.g. "5 Repeated" is present in stdout after the timeout, then we correctly polled for new logs
    ctx.icp()
        .current_dir(&project_dir)
        .timeout(Duration::from_secs(7))
        .args([
            "canister",
            "logs",
            "logger",
            "--follow",
            "--interval",
            "1",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure() // Will timeout/be interrupted
        .stdout(contains("1 Repeated"))
        .stdout(contains("2 Repeated"))
        .stdout(contains("3 Repeated"))
        .stdout(contains("4 Repeated"))
        .stdout(contains("5 Repeated"));
}

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_follow_mode_json() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    // Copy the logger canister assets
    ctx.copy_asset_dir("canister_logs", &project_dir);

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "logger", "--environment", "random-environment"])
        .assert()
        .success();

    // Trigger repeated logging (will log 5 times over 5 seconds)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "logger",
            "log_repeated",
            "(\"Repeated\")",
        ])
        .assert()
        .success();

    // Follow with --json until the timeout kills the process. Because the process is
    // killed rather than exiting normally, nothing buffered in stdout is flushed on the
    // way out: any output we observe here must have been flushed as the records arrived.
    let stdout = ctx
        .icp()
        .current_dir(&project_dir)
        .timeout(Duration::from_secs(7))
        .args([
            "canister",
            "logs",
            "logger",
            "--follow",
            "--json",
            "--interval",
            "1",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure() // Will timeout/be interrupted
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).expect("stdout is not valid UTF-8");

    // Newline-delimited JSON: every line is one complete, parseable record.
    let mut contents = Vec::new();
    for line in stdout.lines() {
        let record: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("--follow --json line {line:?} is not valid JSON: {e}"));
        assert!(record["timestamp"].is_u64());
        assert!(record["index"].is_u64());
        contents.push(
            record["content"]
                .as_str()
                .expect("--follow --json record should contain a content string")
                .to_string(),
        );
    }

    for i in 1..=5 {
        let message = format!("{i} Repeated");
        assert!(
            contents.iter().any(|c| c.contains(&message)),
            "--follow --json output should contain {message:?}, got {contents:?}"
        );
    }
}

/// A consumer that stops reading — `icp canister logs --follow | head -1` — must not make
/// the command report a broken pipe. Runs both output modes: `--json` writes far more often
/// now that it streams, so it reaches the closed pipe first, but the human-readable path
/// closes over exactly the same write.
#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_follow_broken_pipe() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    // Copy the logger canister assets
    ctx.copy_asset_dir("canister_logs", &project_dir);

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "logger", "--environment", "random-environment"])
        .assert()
        .success();

    let log = |message: &str| {
        ctx.icp()
            .current_dir(&project_dir)
            .args([
                "canister",
                "call",
                "--environment",
                "random-environment",
                "logger",
                "log",
                &format!("(\"{message}\")"),
            ])
            .assert()
            .success();
    };

    for json in [true, false] {
        // One log exists before the follow starts, so the first poll has exactly one record
        // to emit and we are not racing the canister for it.
        log(&format!("Before follow {json}"));

        let mut args = vec![
            "canister",
            "logs",
            "logger",
            "--follow",
            "--interval",
            "1",
            "--environment",
            "random-environment",
        ];
        if json {
            args.push("--json");
        }
        let mut child = ctx
            .icp_std()
            .current_dir(&project_dir)
            .args(&args)
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn icp canister logs --follow");

        // Read one record, then close the read end of the pipe, exactly as `head -1` does.
        let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("failed to read the first record");
        assert!(
            !line.is_empty(),
            "expected a first record before the pipe closed"
        );
        drop(reader);

        // Produce another record so the next poll must write to the now-closed pipe. Without
        // this the loop could idle forever and never notice the pipe is gone.
        log(&format!("After close {json}"));

        let status = child.wait().expect("failed to wait for icp canister logs");
        assert!(
            status.success(),
            "a closed stdout pipe should end `--follow` quietly (json: {json}), got {status}"
        );
    }
}

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_filter_by_index() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    ctx.copy_asset_dir("canister_logs", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "logger", "--environment", "random-environment"])
        .assert()
        .success();

    // Create several log entries
    for i in 0..=2 {
        ctx.icp()
            .current_dir(&project_dir)
            .args([
                "canister",
                "call",
                "--environment",
                "random-environment",
                "logger",
                "log",
                &format!("(\"Message {i}\")"),
            ])
            .assert()
            .success();
    }

    // Fetch all logs to verify baseline
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("Message 0")
                .and(contains("Message 1"))
                .and(contains("Message 2")),
        );

    // --since-index is inclusive, so --since-index 1 should include Message 1 and Message 2 but not Message 0
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--since-index",
            "1",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Message 0").not())
        .stdout(contains("Message 1"))
        .stdout(contains("Message 2"));

    // --until-index is exclusive, so --until-index 1 should only include Message 0
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--until-index",
            "1",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Message 0"))
        .stdout(contains("Message 1").not())
        .stdout(contains("Message 2").not());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_filter_by_timestamp() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    ctx.copy_asset_dir("canister_logs", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "logger", "--environment", "random-environment"])
        .assert()
        .success();

    // Create a log entry
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "logger",
            "log",
            "(\"Timestamped message\")",
        ])
        .assert()
        .success();

    // Filter with --since far in the future should return no logs
    // Use a large but valid u64 nanosecond value (~year 2286)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--since",
            "9999999999999999999",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Timestamped message").not());

    // Filter with --since 0 should return all logs
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--since",
            "0",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Timestamped message"));

    // RFC3339 timestamp: --since with a past date should include the log
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--since",
            "2020-01-01T00:00:00Z",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Timestamped message"));
}

// Ignored: fetch_canister_logs is not yet available in replicated mode.
// Tracking: https://github.com/dfinity/portal/pull/6106
#[ignore]
#[cfg(unix)] // moc
#[tokio::test]
async fn canister_logs_through_proxy() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("canister_logs");

    ctx.copy_asset_dir("canister_logs", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: logger
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy logger through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "logger",
            "--proxy",
            &proxy_cid,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Create some logs by calling through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "logger",
            "log",
            "(\"Proxy log message\")",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success();

    // Fetch logs through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "logs",
            "logger",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(contains("Proxy log message"));
}
