#[cfg(unix)]
use {
    crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext},
    icp::fs::write_string,
    indoc::formatdoc,
    predicates::prelude::PredicateBooleanExt,
    predicates::str::contains,
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

    // Fetch logs
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
        .stdout(contains("Test message 1"))
        .stdout(contains("Test message 2"));
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
