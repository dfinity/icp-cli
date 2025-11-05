use icp::{
    fs::{create_dir_all, write_string},
    prelude::*,
};
use indoc::formatdoc;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};

mod common;

#[test]
fn sync_adapter_script_single() {
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
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Deploy project (it should sync as well)
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "--debug",
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stdout(contains("syncing").trim());

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args(["--debug", "sync", "--environment", "my-environment"])
        .assert()
        .success()
        .stdout(contains("syncing").trim());
}

#[test]
fn sync_adapter_script_multiple() {
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
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "second"
                - type: script
                  command: echo "first"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Deploy project (it should sync as well)
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "--debug",
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stdout(contains("first").and(contains("second")));

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args(["--debug", "sync", "--environment", "my-environment"])
        .assert()
        .success()
        .stdout(contains("first").and(contains("second")));
}

#[tokio::test]
async fn sync_adapter_static_assets() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");
    let assets_dir = project_dir.join("www");

    // Create assets directory
    create_dir_all(&assets_dir).expect("failed to create assets directory");

    // Create simple index page
    write_string(&assets_dir.join("index.html"), "hello").expect("failed to create index page");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  url: https://github.com/dfinity/sdk/raw/refs/tags/0.27.0/src/distributed/assetstorage.wasm.gz
                  sha256: 865eb25df5a6d857147e078bb33c727797957247f7af2635846d65c5397b36a6

            sync:
              steps:
                - type: assets
                  dirs:
                    - {assets_dir}

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "my-network");

    let network_port = ctx
        .wait_for_network_descriptor(&project_dir, "my-network")
        .gateway_port;

    // Canister ID
    let cid = "tqzl2-p7777-77776-aaaaa-cai";

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args(["sync", "--environment", "my-environment"])
        .assert()
        .success();

    // Verify that assets canister was synced
    let resp = reqwest::get(format!("http://localhost:{network_port}/?canisterId={cid}"))
        .await
        .expect("request failed");

    let out = resp
        .text()
        .await
        .expect("failed to read canister response body");

    assert_eq!(out, "hello");
}
