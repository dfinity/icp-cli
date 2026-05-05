use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};
use test_tag::tag;

use crate::common::{
    ENVIRONMENT_DOCKER_ENGINE, ENVIRONMENT_RANDOM_PORT, NETWORK_DOCKER_ENGINE, NETWORK_RANDOM_PORT,
    TestContext, clients,
};
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

#[tokio::test]
async fn canister_create_with_fixed_controller_principals() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // "aaaaa-aa" is the management canister principal — a convenient fixed value.
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
            settings:
              controllers:
                - "aaaaa-aa"

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
        ])
        .assert()
        .success();

    // The controller list must include both the declared principal and the active identity
    // (2vxsx-fae = anonymous principal). The greenfield injection ensures the caller retains
    // access even when manifest controllers are set.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("Controllers:")
                .and(contains("aaaaa-aa"))
                .and(contains("2vxsx-fae")),
        );
}

#[tokio::test]
async fn canister_create_with_resolved_canister_controller() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Canister "a" lists "b" as a controller by name.
    let pm = formatdoc! {r#"
        canisters:
          - name: a
            build:
              steps:
                - type: script
                  command: echo hi
            settings:
              controllers:
                - b
          - name: b
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

    let client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    client.mint_cycles(100 * TRILLION);

    // Create "b" first so that when "a" is created the controller reference resolves immediately.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "b",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    let b_principal = client.get_canister_id("b").to_string();

    // Creating "a" should produce no warning: "b" is already on-chain.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "a",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stderr(contains("does not exist yet").not());

    // "a"'s controllers must include "b"'s principal and the active identity (2vxsx-fae).
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "a",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("Controllers:")
                .and(contains(b_principal.as_str()))
                .and(contains("2vxsx-fae")),
        );
}

#[tokio::test]
async fn canister_create_with_unresolved_canister_controller_warns_and_syncs() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Canister "a" lists "b" as a controller by name.
    let pm = formatdoc! {r#"
        canisters:
          - name: a
            build:
              steps:
                - type: script
                  command: echo hi
            settings:
              controllers:
                - b
          - name: b
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

    let client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    client.mint_cycles(100 * TRILLION);

    // Create "a" before "b" — "b" is unresolved, so a warning must be emitted.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "a",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stderr(contains(
            "Controller canister 'b' for 'a' does not exist yet",
        ));

    // At this point "a" is only controlled by the active identity; "b" is not yet a controller.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "a",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Controllers: 2vxsx-fae"));

    // Creating "b" triggers sync_controller_dependents, which updates "a"'s controller list.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "b",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    let b_principal = client.get_canister_id("b").to_string();

    // After "b" is created, "a"'s controllers must include "b"'s principal.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "a",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("Controllers:")
                .and(contains(b_principal.as_str()))
                .and(contains("2vxsx-fae")),
        );
}

#[tag(docker)]
#[tokio::test]
async fn canister_create_cloud_engine() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi

        {NETWORK_DOCKER_ENGINE}
        {ENVIRONMENT_DOCKER_ENGINE}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.docker_pull_engine_network();
    let _guard = ctx
        .start_network_in(&project_dir, "docker-engine-network")
        .await;
    ctx.ping_until_healthy(&project_dir, "docker-engine-network");

    // Find the CloudEngine subnet by querying the topology endpoint
    // TODO replace with a subnet selection parameter once we have one
    let topology_url = ctx.gateway_url().join("/_/topology").unwrap();
    let topology: serde_json::Value = reqwest::get(topology_url)
        .await
        .expect("failed to fetch topology")
        .json()
        .await
        .expect("failed to parse topology");

    let subnet_configs = topology["subnet_configs"]
        .as_object()
        .expect("subnet_configs should be an object");
    let cloud_engine_subnet_id = subnet_configs
        .iter()
        .find_map(|(id, config)| {
            (config["subnet_kind"].as_str()? == "CloudEngine").then_some(id.clone())
        })
        .expect("no CloudEngine subnet found in topology");

    // Create the canister on the CloudEngine subnet
    // Only the admin can do this. In local envs, the admin is the anonymous principal
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--subnet",
            &cloud_engine_subnet_id,
            "--environment",
            "docker-engine-environment",
        ])
        .assert()
        .success();

    let id_mapping_path = project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("docker-engine-environment.ids.json");
    assert!(
        id_mapping_path.exists(),
        "ID mapping file should exist at {id_mapping_path}"
    );
}
