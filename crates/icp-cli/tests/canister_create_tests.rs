use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[test]
fn canister_create() {
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
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    let id_mapping_path = project_dir.join(".icpdata").join("my-environment.ids.json");
    assert!(
        id_mapping_path.exists(),
        "ID mapping file should exist at {id_mapping_path}"
    );
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
            canisters:
              - name: my-canister
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
            "my-canister",
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
        .stderr(
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
            "my-canister",
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
        .stderr(
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
