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
