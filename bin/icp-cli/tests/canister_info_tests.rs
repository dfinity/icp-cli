use crate::common::TestEnv;
use icp_fs::fs::write;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use serial_test::serial;

mod common;

#[test]
#[serial]
fn canister_status() {
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
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        wasm,
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

    // Query status
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "info", "my-canister"])
        .assert()
        .success()
        .stderr(contains("Controllers: 2vxsx-fae").and(contains(
            "Module hash: 0x17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a",
        )));
}
