use icp::{
    fs::{create_dir_all, write_string},
    prelude::*,
    store_id::IdMapping,
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
    let _g = ctx.start_network_in(&project_dir, "random-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project (it should sync as well)
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "--debug",
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("syncing").trim());

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args([
            "--debug",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
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
    let _g = ctx.start_network_in(&project_dir, "random-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project (it should sync as well)
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "--debug",
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("first").and(contains("second")));

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args([
            "--debug",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
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
    let _g = ctx.start_network_in(&project_dir, "random-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    let network_port = ctx
        .wait_for_network_descriptor(&project_dir, "random-network")
        .gateway_port;

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    let id_mapping_path = project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("random-environment.ids.json");

    let id_mapping_content: IdMapping =
        icp::fs::json::load(&id_mapping_path).expect("failed to read ID mapping file");

    let cid = id_mapping_content
        .get("my-canister")
        .expect("canister ID not found");

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args(["sync", "my-canister", "--environment", "random-environment"])
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

#[test]
fn sync_with_valid_principal() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
            sync:
              steps:
                - type: script
                  command: echo syncing
        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network");
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Valid principal
    let principal = "aaaaa-aa";

    // Try to sync with principal (should fail)
    ctx.icp()
        .current_dir(&project_dir)
        .args(["sync", principal, "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(contains("project does not contain a canister named"));
}

#[test]
fn sync_multiple_canisters() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with multiple canisters
    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-a"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-b"
          - name: canister-c
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-c"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Sync multiple canisters
    ctx.icp()
        .current_dir(project_dir)
        .env("NO_COLOR", "1")
        .args([
            "--debug",
            "sync",
            "canister-a",
            "canister-b",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Syncing canisters"))
        .stdout(contains(r#"canisters: ["canister-a", "canister-b"]"#))
        .stdout(contains("DEBUG icp::progress: syncing canister-a"))
        .stdout(contains("DEBUG icp::progress: syncing canister-b"))
        .stdout(contains("DEBUG icp::progress: syncing canister-c").not());
}

#[test]
fn sync_all_canisters_in_environment() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with multiple canisters and environments
    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-a"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-b"
          - name: canister-c
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-c"

        {NETWORK_RANDOM_PORT}
        
        environments:
          - name: test-env
            network: random-network
            canisters:
              - canister-a
              - canister-b
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("test-env".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "test-env",
        ])
        .assert()
        .success();

    // Sync all canisters in environment (no canister names specified)
    ctx.icp()
        .current_dir(project_dir)
        .env("NO_COLOR", "1")
        .args(["--debug", "sync", "--environment", "test-env"])
        .assert()
        .success()
        .stdout(contains("Syncing canisters"))
        .stdout(contains(r#"canisters: []"#))
        .stdout(contains(r#"environment: Some("test-env")"#))
        .stdout(contains("DEBUG icp::progress: syncing canister-a"))
        .stdout(contains("DEBUG icp::progress: syncing canister-b"))
        .stdout(contains("DEBUG icp::progress: syncing canister-c").not()); // not in test-env
}
