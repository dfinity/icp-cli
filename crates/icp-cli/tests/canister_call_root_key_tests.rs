use indoc::formatdoc;
use predicates::ord::eq;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{PredicateStrExt, contains};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext};
use icp::fs::write_string;

mod common;

#[tokio::test]
async fn canister_call_with_url_and_root_key() {
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

    // Get the network information so we can call the network directly
    let assert = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "network",
            "status",
            "--environment",
            "random-environment",
            "--json",
        ])
        .assert()
        .success();
    let output = assert.get_output();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let gateway_url = json["gateway_url"].as_str().expect("Should be a string");
    let root_key = json["root_key"].as_str().expect("Should be a string");

    // Get the canister information so we can call the network directly
    let assert = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "--environment",
            "random-environment",
            "my-canister",
            "--id-only",
        ])
        .assert()
        .success();

    let output = assert.get_output();
    let canister_id =
        String::from_utf8(output.stdout.clone()).expect("canister id should be a valid string");
    let canister_id = canister_id.trim();

    // Test calling with with url from external directory
    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            gateway_url,
            "--root-key",
            root_key,
            canister_id,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Test calling with with url from external directory with bad root key
    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            gateway_url,
            "--root-key",
            "badbadbad", // This is an invalid root key
            canister_id,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .failure()
        .stderr(contains("invalid value 'badbadbad' for '--root-key"));
}
