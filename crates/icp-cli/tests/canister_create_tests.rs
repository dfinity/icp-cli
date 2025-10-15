use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use pocket_ic::common::rest::{InstanceConfig, SubnetConfigSet};
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, network::managed::pocketic::default_instance_config, prelude::*};

mod common;

#[test]
fn canister_create() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
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
    let _g = ctx.start_network_in(&project_dir, "my-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Create canister
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create", "--environment", "my-environment"])
        .assert()
        .success();
}

#[test]
fn canister_create_with_settings() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
            canister:
              name: my-canister
              build:
                steps:
                  - type: script
                    command: cp {path} "$ICP_WASM_OUTPUT_PATH"
              settings:
                compute_allocation: 1
                memory_allocation: 4294967296
                freezing_threshold: 2592000
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
    let _g = ctx.start_network_in(&project_dir, "my-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Create canister
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--environment",
            "my-environment",
            "--cycles",
            &format!("{}", 70 * TRILLION), /* 70 TCYCLES because compute allocation is expensive */
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
            "my-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 1"))
                .and(contains("Memory allocation: 4_294_967_296"))
                .and(contains("Freezing threshold: 2_592_000"))
                .and(contains("Reserved cycles limit: 1_000_000_000_000")),
        );
}

#[test]
fn canister_create_with_settings_cmdline_override() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
            canister:
              name: my-canister
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
    let _g = ctx.start_network_in(&project_dir, "my-network");

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Create canister
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--compute-allocation",
            "2",
            "--environment",
            "my-environment",
            "--cycles",
            &format!("{}", 70 * TRILLION), /* 70 TCYCLES because compute allocation is expensive */
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
            "my-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 2")),
        );
}

#[test]
fn canister_create_nonexistent_canister() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with canister named "a"
    let pm = indoc! {r#"
        canister:
          name: a
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
        .stderr(contains("project does not contain a canister named 'b'"));
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
            "environment 'test-env' does not include canister 'b'",
        ));
}

#[tokio::test]
async fn canister_create_colocates_canisters() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = indoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-b
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-c
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-d
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-e
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-f
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

    // Start network
    let _g = ctx
        .start_network_with_config(
            &project_dir,
            InstanceConfig {
                subnet_config_set: (SubnetConfigSet {
                    application: 3,
                    ..Default::default()
                })
                .into(),
                ..default_instance_config(&ctx.state_dir(&project_dir))
            },
        )
        .await;

    ctx.ping_until_healthy(&project_dir, "local");

    // Create first three canisters
    let icp_client = clients::icp(&ctx, &project_dir, None);
    icp_client.mint_cycles(20 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "canister-a",
            "canister-b",
            "canister-c",
        ])
        .assert()
        .success();

    let registry = clients::registry(&ctx);

    let subnet_a = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-a"))
        .await;

    let subnet_b = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-b"))
        .await;

    let subnet_c = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-c"))
        .await;

    assert_eq!(
        subnet_a, subnet_b,
        "Canister A and B should be on the same subnet"
    );
    assert_eq!(
        subnet_a, subnet_c,
        "Canister B and C should be on the same subnet"
    );

    // Create remaining canisters
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "canister-d",
            "canister-e",
            "canister-f",
        ])
        .assert()
        .success();

    let subnet_d = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-d"))
        .await;

    let subnet_e = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-e"))
        .await;

    let subnet_f = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-f"))
        .await;

    assert_eq!(
        subnet_a, subnet_d,
        "Canister D should be on the same subnet as canister A"
    );
    assert_eq!(
        subnet_a, subnet_e,
        "Canister E should be on the same subnet as canister A"
    );
    assert_eq!(
        subnet_a, subnet_f,
        "Canister F should be on the same subnet as canister A"
    );
}
