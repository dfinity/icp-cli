use indoc::formatdoc;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[test]
fn canister_start() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

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

    // Stop canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "my-canister",
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
            "status",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Stopped"))
                .and(contains("Controllers: 2vxsx-fae")),
        );

    // Start canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "start",
            "my-canister",
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
                .and(contains("Controllers: 2vxsx-fae")),
        );
}
