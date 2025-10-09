use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};
use predicates::{prelude::PredicateBooleanExt, str::contains};

mod common;

#[test]
fn canister_status() {
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

{NETWORK_RANDOM_PORT}
{ENVIRONMENT_RANDOM_PORT}
        "#,
        wasm,
    );

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "info",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(contains("Controllers: 2vxsx-fae").and(contains(
            "Module hash: 0x17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a",
        )));
}
