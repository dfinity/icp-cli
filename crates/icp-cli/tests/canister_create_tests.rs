use crate::common::{TestContext, clients};
use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use icp::{fs::write_string, prelude::*};
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

mod common;

#[test]
fn canister_create() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canister:
      name: my-canister
      build:
        steps:
          - type: script
            command: echo hi
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    clients::icp(&ctx, &project_dir).mint_cycles(100 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create"])
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
            compute_allocation: 1
            memory_allocation: 4294967296
            freezing_threshold: 2592000
            reserved_cycles_limit: 1000000000000
        "#,
        f.path()
    );

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    clients::icp(&ctx, &project_dir).mint_cycles(100 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--cycles",
            &format!("{}", 70 * TRILLION), /* 70 TCYCLES because compute allocation is expensive */
        ])
        .assert()
        .success();

    // Verify creation settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
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
            compute_allocation: 1
        "#,
        f.path()
    );

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    clients::icp(&ctx, &project_dir).mint_cycles(100 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--compute-allocation",
            "2",
            "--cycles",
            &format!("{}", 70 * TRILLION), /* 70 TCYCLES because compute allocation is expensive */
        ])
        .assert()
        .success();

    // Verify creation settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
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

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with canister named "a"
    let pm = r#"
    canister:
      name: a
      build:
        steps:
          - type: script
            command: echo hi
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Try to create canister "b" which doesn't exist in the project
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

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with canisters "a" and "b", but environment only includes "a"
    let pm = r#"
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
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Try to create canister "b" which is not included in the environment
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create", "b", "--environment", "test-env"])
        .assert()
        .failure()
        .stderr(contains(
            "environment 'test-env' does not include canister 'b'",
        ));
}
