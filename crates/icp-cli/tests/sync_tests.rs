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

#[tokio::test]
async fn sync_adapter_script_single() {
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
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

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
        .stderr(contains("syncing").trim());

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
        .stderr(contains("syncing").trim());
}

#[tokio::test]
async fn sync_adapter_script_multiple() {
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
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

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
        .stderr(contains("first").and(contains("second")));

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
        .stderr(contains("first").and(contains("second")));
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
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

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

    // Verify that assets canister was synced via canisterId query param
    let resp = reqwest::get(format!("http://localhost:{network_port}/?canisterId={cid}"))
        .await
        .expect("request failed");

    let out = resp
        .text()
        .await
        .expect("failed to read canister response body");

    assert_eq!(out, "hello");

    // Verify that the friendly domain also works
    let friendly_domain = "my-canister.random-environment.localhost";
    let client = reqwest::Client::builder()
        .resolve(
            friendly_domain,
            std::net::SocketAddr::from(([127, 0, 0, 1], network_port)),
        )
        .build()
        .expect("failed to build reqwest client");
    let resp = client
        .get(format!("http://{friendly_domain}:{network_port}/"))
        .send()
        .await
        .expect("friendly domain request failed");
    let out = resp
        .text()
        .await
        .expect("failed to read friendly domain response body");
    assert_eq!(out, "hello");
}

#[tokio::test]
async fn sync_with_valid_principal() {
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
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
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

#[tokio::test]
async fn sync_multiple_canisters() {
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
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-a"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-b"
          - name: canister-c
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
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
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

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
        .stderr(contains("Syncing canisters"))
        .stderr(contains(r#"canisters: ["canister-a", "canister-b"]"#))
        .stderr(contains("DEBUG icp::progress: syncing canister-a"))
        .stderr(contains("DEBUG icp::progress: syncing canister-b"))
        .stderr(contains("DEBUG icp::progress: syncing canister-c").not());
}

/// Compiles the canister and plugin from `examples/icp-sync-plugin/` and returns
/// (canister_wasm_path, plugin_wasm_path). Cargo caches the build so subsequent
/// test runs are fast when sources haven't changed.
fn build_sync_plugin_example() -> (PathBuf, PathBuf) {
    let example_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/icp-sync-plugin");
    // Use CARGO env var when available (set by cargo test), fall back to PATH lookup.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let status = std::process::Command::new(&cargo)
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "-p",
            "canister",
        ])
        .current_dir(&example_dir)
        .status()
        .expect("failed to spawn cargo build for canister");
    assert!(
        status.success(),
        "cargo build --target wasm32-unknown-unknown failed"
    );

    let status = std::process::Command::new(&cargo)
        .args([
            "build",
            "--target",
            "wasm32-wasip2",
            "--release",
            "-p",
            "plugin",
        ])
        .current_dir(&example_dir)
        .status()
        .expect("failed to spawn cargo build for plugin");

    assert!(
        status.success(),
        "cargo build --target wasm32-wasip2 failed"
    );

    (
        example_dir.join("target/wasm32-unknown-unknown/release/canister.wasm"),
        example_dir.join("target/wasm32-wasip2/release/plugin.wasm"),
    )
}

#[tokio::test]
async fn sync_plugin_registers_seed_data() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // Create seed-data directory with fruit files
    let seed_data = project_dir.join("seed-data");
    create_dir_all(&seed_data).expect("failed to create seed-data");
    write_string(&seed_data.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");
    write_string(&seed_data.join("fruit-02.txt"), "banana").expect("failed to write fruit-02.txt");
    write_string(&seed_data.join("fruit-03.txt"), "cherry").expect("failed to write fruit-03.txt");

    // Manifest: pre-built canister wasm + plugin sync step pointing at the pre-built plugin wasm.
    // dirs is relative to the project directory and preopened read-only inside the plugin's WASI sandbox.
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Mint cycles and deploy. deploy also runs the sync step: the plugin calls
    // set_uploader (user is controller, so the direct call is permitted), then
    // calls register for each fruit file directly with the user identity as the uploader.
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

    // Query the canister to verify all three fruits were registered
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "show",
            "()",
            "--query",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("apple")
                .and(contains("banana"))
                .and(contains("cherry")),
        );
}

#[tokio::test]
async fn sync_plugin_routes_through_proxy() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // Create seed-data directory with fruit files
    let seed_data = project_dir.join("seed-data");
    create_dir_all(&seed_data).expect("failed to create seed-data");
    write_string(&seed_data.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");
    write_string(&seed_data.join("fruit-02.txt"), "banana").expect("failed to write fruit-02.txt");
    write_string(&seed_data.join("fruit-03.txt"), "cherry").expect("failed to write fruit-03.txt");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network (the proxy canister is automatically deployed)
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy through proxy so the proxy canister becomes a controller of my-canister.
    // deploy also runs the sync step: the plugin routes set_uploader through the proxy
    // (direct: false, proxy is controller), then calls register directly with the user identity.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--proxy",
            &proxy_cid,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query the canister to verify all three fruits were registered
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "show",
            "()",
            "--query",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("apple")
                .and(contains("banana"))
                .and(contains("cherry")),
        );
}

#[tokio::test]
async fn sync_all_canisters_in_environment() {
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
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-a"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-b"
          - name: canister-c
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
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
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

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
        .stderr(contains("Syncing canisters"))
        .stderr(contains(r#"canisters: []"#))
        .stderr(contains(r#"environment: Some("test-env")"#))
        .stderr(contains("DEBUG icp::progress: syncing canister-a"))
        .stderr(contains("DEBUG icp::progress: syncing canister-b"))
        .stderr(contains("DEBUG icp::progress: syncing canister-c").not()); // not in test-env
}
