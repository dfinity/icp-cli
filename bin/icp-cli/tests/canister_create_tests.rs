use crate::common::{TestContext, TestNetwork};
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
    let ctx = TestContext::new().with_dfx();

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

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    ctx.icp()
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
    let ctx = TestContext::new().with_dfx();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

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
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    ctx.icp()
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
    ctx.icp()
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
    let ctx = TestContext::new().with_dfx();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

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
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    ctx.icp()
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
    ctx.icp()
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

#[test]
#[serial]
fn canister_create_via_cycles_ledger() {
    let ctx = TestContext::new().with_dfx();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);
    let TestNetwork {
        gateway_port,
        root_key,
        ..
    } = ctx.wait_for_local_network_descriptor(&project_dir);

    // Project manifest
    let pm = format!(
        r#"
        canister:
        name: my-canister
        build:
            steps:
            - type: script
              command: echo hi
        
        networks:
            - name: ic
              mode: connected
              url: http://localhost:{gateway_port}
              root-key: "{root_key}"

        environments:
            - name: ic
              network: ic
        "#
    );

    write(
        project_dir.join("icp.yaml"), // path
        &pm,                          // contents
    )
    .expect("failed to write project manifest");

    println!("icp.yaml: {}", pm);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    ctx.icp_().use_new_random_identity();
    let principal = ctx.icp_().active_principal();
    ctx.icp_ledger()
        .mint_icp(principal, None, 10_000_000_000_000_u64);
    assert_eq!(
        ctx.icp_ledger().balance_of(principal, None),
        10_000_000_000_000_u64
    );
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--ic"])
        .assert()
        .stdout(contains("10000000000000"))
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args(["identity", "principal"])
        .assert()
        .stdout(contains(principal.to_string()))
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--cycles-amount",
            "2000000000000", /* 2T cycles */
            "--ic",
        ])
        .assert()
        .success();
    let balance_before = ctx.cycles_ledger().balance_of(principal, None);

    // Create canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
            "--cycles",
            "1000000000000",
            "--ic",
        ])
        .assert()
        .success();
    let balance_after = ctx.cycles_ledger().balance_of(principal, None);
    assert!(balance_after < balance_before);
}
