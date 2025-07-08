use crate::common::TestEnv;
use icp_fs::fs::write;
use predicates::{ord::eq, str::PredicateStrExt};
use serial_test::serial;

mod common;

#[test]
#[serial]
fn sync_adapter_script_single() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: sh -c 'cp {wasm} "$ICP_WASM_OUTPUT_PATH"'
          sync:
            adapter:
              type: script
              command: echo "syncing"
        "#,
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

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Invoke sync
    env.icp()
        .current_dir(project_dir)
        .args(["sync"])
        .assert()
        .success()
        .stdout(eq("syncing").trim());
}

#[test]
#[serial]
fn sync_adapter_script_multiple() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: sh -c 'cp {wasm} "$ICP_WASM_OUTPUT_PATH"'
          sync:
            - adapter:
                type: script
                command: echo "first"
            - adapter:
                type: script
                command: echo "second"
        "#,
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

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["sync"])
        .assert()
        .success()
        .stdout(eq("first\nsecond").trim());
}
