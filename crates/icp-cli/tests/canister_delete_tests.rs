use indoc::formatdoc;
use predicates::str::contains;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[test]
fn canister_delete() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
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
    let _g = ctx.start_network_in(&project_dir, "random-network");
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify canister ID is in the id store after deploy
    let id_mapping_path = project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("random-environment.ids.json");
    let id_mapping_before =
        std::fs::read_to_string(&id_mapping_path).expect("ID mapping file should exist");
    assert!(
        id_mapping_before.contains("my-canister"),
        "ID mapping should contain my-canister before deletion"
    );

    // Stop canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Delete canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "delete",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify canister ID is removed from the id store after delete
    let id_mapping_after =
        std::fs::read_to_string(&id_mapping_path).expect("ID mapping file should still exist");
    assert!(
        !id_mapping_after.contains("my-canister"),
        "ID mapping should NOT contain my-canister after deletion"
    );

    // Query status - should fail because canister ID is not found in id store
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("could not find ID for canister"));
}
