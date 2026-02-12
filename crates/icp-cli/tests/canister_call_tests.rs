use indoc::formatdoc;
use predicates::ord::eq;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{PredicateStrExt, contains};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext};
use icp::fs::write_string;

mod common;

#[tokio::test]
async fn canister_call_with_arguments() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

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
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Test calling with Candid text format
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Test calling with hex-encoded arguments
    // This is the hex encoding of Candid "(\"world\")" - didc encode '("world")'
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "4449444c00017105776f726c64",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Test calling with --query flag (greet is a query method in the Candid interface)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--query",
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());
}

#[tokio::test]
async fn canister_call_with_arguments_from_file() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Create argument files
    write_string(&project_dir.join("args_candid.txt"), "(\"world\")")
        .expect("failed to write candid args file");

    // Hex encoding of Candid "(\"world\")" - didc encode '("world")'
    write_string(
        &project_dir.join("args_hex.txt"),
        "4449444c00017105776f726c64",
    )
    .expect("failed to write hex args file");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Test calling with Candid arguments from file
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "args_candid.txt",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Test calling with hex arguments from file
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "args_hex.txt",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Test with absolute path
    let abs_path = project_dir.join("args_candid.txt");
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            abs_path.as_str(),
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());
}

#[tokio::test]
async fn canister_call_through_proxy() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let target_wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with target canister
    let pm = formatdoc! {r#"
        canisters:
          - name: target
            build:
              steps:
                - type: script
                  command: cp '{target_wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network (automatically installs proxy canister)
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy target canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "target", "--environment", "random-environment"])
        .assert()
        .success();

    // Get the proxy canister ID from network status
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network", "--json"])
        .output()
        .expect("failed to get network status");
    let status_json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("failed to parse network status JSON");
    let proxy_cid = status_json
        .get("proxy_canister_principal")
        .and_then(|v| v.as_str())
        .expect("proxy canister principal not found in network status")
        .to_string();

    // Test calling target canister through the proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "target",
            "greet",
            "(\"proxy\")",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, proxy!\")").trim());

    // Test calling through proxy with cycles (should also work with 0 cycles)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "target",
            "greet",
            "(\"world\")",
            "--proxy",
            &proxy_cid,
            "--cycles",
            "0",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());
}

#[tokio::test]
async fn canister_call_query_conflicts_with_proxy() {
    let ctx = TestContext::new();

    // --query and --proxy conflict at the clap level, so no network setup is needed.
    ctx.icp()
        .args([
            "canister",
            "call",
            "--query",
            "--proxy",
            "aaaaa-aa",
            "some-canister",
            "some-method",
        ])
        .assert()
        .failure()
        .stderr(contains("--query").and(contains("--proxy")));
}
