use crate::common::TestEnv;
use camino_tempfile::NamedUtf8TempFile;
use icp_fs::fs::write;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};
use serial_test::serial;

mod common;

#[test]
#[serial]
fn canister_create() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canister:
      name: my-canister
      build:
        steps:
          - type: script
            command: echo hi
    "#;

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.ping_until_healthy(&project_dir);

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();
}

#[test]
#[serial]
fn canister_create_with_settings() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
          settings:
            compute_allocation: 10
            memory_allocation: 4294967296
            freezing_threshold: 2592000
            reserved_cycles_limit: 1000000000000
            wasm_memory_limit: 1073741824
            wasm_memory_threshold: 536870912
        "#,
        f.path()
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.ping_until_healthy(&project_dir);

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();

    // Verify creation settings
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 10"))
                .and(contains("Memory allocation: 4_294_967_296"))
                .and(contains("Freezing threshold: 2_592_000"))
                .and(contains("Reserved cycles limit: 1_000_000_000_000"))
                .and(contains("Wasm memory limit: 1_073_741_824"))
                .and(contains("Wasm memory threshold: 536_870_912")),
        );
}

#[test]
#[serial]
fn canister_create_with_settings_cmdline_override() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
          settings:
            compute_allocation: 10
        "#,
        f.path()
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.ping_until_healthy(&project_dir);

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
            "--compute-allocation",
            "20",
        ])
        .assert()
        .success();

    // Verify creation settings
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 20")),
        );
}
