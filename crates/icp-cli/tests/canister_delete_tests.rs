use crate::common::{TRILLION, TestContext, clients};
use icp_fs::fs::write;

mod common;

#[test]
fn canister_delete() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    clients::icp(&ctx, &project_dir).mint_cycles(10 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Stop canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "stop", "my-canister"])
        .assert()
        .success();

    // Delete canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "delete", "my-canister"])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .failure();
}
