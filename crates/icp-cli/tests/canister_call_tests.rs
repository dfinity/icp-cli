use indoc::formatdoc;
use predicates::{ord::eq, str::PredicateStrExt};

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

    // Test calling with --json output
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--json",
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("\"Hello, world!\"").trim());
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

    // Use vendored WASMs
    let target_wasm = ctx.make_asset("example_icp_mo.wasm");
    let proxy_wasm = ctx.make_asset("proxy.wasm");

    // Project manifest with both target and proxy canisters
    let pm = formatdoc! {r#"
        canisters:
          - name: target
            build:
              steps:
                - type: script
                  command: cp '{target_wasm}' "$ICP_WASM_OUTPUT_PATH"

          - name: proxy
            build:
              steps:
                - type: script
                  command: cp '{proxy_wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy both canisters
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Get the proxy canister ID using canister status --id-only
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "--environment",
            "random-environment",
            "--id-only",
            "proxy",
        ])
        .output()
        .expect("failed to get proxy canister id");
    let proxy_cid = String::from_utf8(output.stdout)
        .expect("invalid utf8")
        .trim()
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
