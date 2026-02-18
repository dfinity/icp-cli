use indoc::formatdoc;
use predicates::str::contains;
use std::time::Duration;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext};
use icp::fs::write_string;

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
