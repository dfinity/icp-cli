use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[tokio::test]
async fn canister_create() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi

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

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
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
    assert!(
        id_mapping_path.exists(),
        "ID mapping file should exist at {id_mapping_path}"
    );
}

#[tokio::test]
async fn canister_create_with_settings() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
            canisters:
              - name: my-canister
                build:
                  steps:
                    - type: script
                      command: cp {path} "$ICP_WASM_OUTPUT_PATH"
                settings:
                  log_visibility: public
                  compute_allocation: 1
                  memory_allocation: 4294967296
                  freezing_threshold: 30d
                  reserved_cycles_limit: 1000000000000

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

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
            "--cycles",
            "70t", /* 70T cycles because compute allocation is expensive */
        ])
        .assert()
        .success();

    // Verify creation settings. Note: log_visibility IS supported by the real cycles ledger
    // during creation, but PocketIC's fake-cmc doesn't implement it. Other settings work fine.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 1"))
                .and(contains("Memory allocation: 4_294_967_296"))
                .and(contains("Freezing threshold: 2_592_000"))
                .and(contains("Reserved cycles limit: 1_000_000_000_000")),
        );

    // Sync settings from manifest to apply log_visibility
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify log_visibility is now applied after sync
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Status: Running"))
                .and(contains("Log visibility: Public")),
        );
}

#[tokio::test]
async fn canister_create_with_settings_suffix_in_yaml() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // reserved_cycles_limit, memory_allocation and wasm_memory_limit with suffixes
    let pm = formatdoc! {r#"
            canisters:
              - name: my-canister
                build:
                  steps:
                    - type: script
                      command: cp {path} "$ICP_WASM_OUTPUT_PATH"
                settings:
                  compute_allocation: 1
                  reserved_cycles_limit: 1.2t
                  memory_allocation: 2gib
                  wasm_memory_limit: 0.25kib

            {NETWORK_RANDOM_PORT}
            {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
            "--cycles",
            "70t",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Status: Running"))
                .and(contains("Reserved cycles limit: 1_200_000_000_000"))
                .and(contains("Memory allocation: 2_147_483_648")),
        );

    // Sync settings from manifest to apply wasm_memory_limit (not sent at create)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Wasm memory limit: 256"));
}

#[tokio::test]
async fn canister_create_with_settings_cmdline_override() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
            canisters:
              - name: my-canister
                build:
                  steps:
                    - type: script
                      command: cp {path} \"$ICP_WASM_OUTPUT_PATH\"
                settings:
                  compute_allocation: 1

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

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--compute-allocation",
            "2",
            "--reserved-cycles-limit",
            "5t",
            "--environment",
            "random-environment",
            "--cycles",
            "70t", /* 70T cycles because compute allocation is expensive */
        ])
        .assert()
        .success();

    // Verify creation settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 2"))
                .and(contains("Reserved cycles limit: 5_000_000_000_000")),
        );
}

#[test]
fn canister_create_nonexistent_canister() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with canister named "a"
    let pm = indoc! {r#"
        canisters:
          - name: a
            build:
              steps:
                - type: script
                  command: echo hi
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create", "b"])
        .assert()
        .failure()
        .stderr(contains("canister 'b' not declared in environment 'local'"));
}

#[test]
fn canister_create_canister_not_in_environment() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = indoc! {r#"
        canisters:
          - name: a
            build:
              steps:
                - type: script
                  command: echo hi
          - name: b
            build:
              steps:
                - type: script
                  command: echo hi

        environments:
          - name: test-env
            network: local
            canisters: [a]
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create", "b", "--environment", "test-env"])
        .assert()
        .failure()
        .stderr(contains(
            "canister 'b' not declared in environment 'test-env'",
        ));
}

#[test]
fn canister_create_with_valid_principal() {
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

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Valid principal
    let principal = "aaaaa-aa";

    // Try to create with principal (should fail)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            principal,
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("Cannot create a canister by principal"));
}

#[tokio::test]
async fn canister_create_detached() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
        {NETWORK_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Get the network information so we can call the network directly
    let assert = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network", "--json"])
        .assert()
        .success();
    let output = assert.get_output();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let gateway_url = json["gateway_url"].as_str().expect("Should be a string");
    let root_key = json["root_key"].as_str().expect("Should be a string");

    // Test creating outside a project
    ctx.icp()
        .args([
            "canister",
            "create",
            "--network",
            gateway_url,
            "--root-key",
            root_key,
            "--detached",
        ])
        .assert()
        .success()
        .stdout(starts_with("Created canister with ID"));

    // Test creating inside a project
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--network",
            "random-network",
            "--detached",
        ])
        .assert()
        .success()
        .stdout(starts_with("Created canister with ID"));

    // Test it fails outside of a project
    ctx.icp()
        .args([
            "canister",
            "create",
            "--network",
            "random-network",
            "--detached",
        ])
        .assert()
        .failure();
}

#[tokio::test]
async fn canister_create_through_proxy() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Get the proxy canister ID from network status
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network", "--json"])
        .output()
        .expect("failed to get network status");
    let status_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("failed to parse network status JSON");
    let proxy_cid = status_json
        .get("proxy_canister_principal")
        .and_then(|v| v.as_str())
        .expect("proxy canister principal not found in network status")
        .to_string();

    // Create canister through the proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(contains("Created canister my-canister with ID"));

    let id_mapping_path = project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("random-environment.ids.json");
    assert!(
        id_mapping_path.exists(),
        "ID mapping file should exist at {id_mapping_path}"
    );
}
