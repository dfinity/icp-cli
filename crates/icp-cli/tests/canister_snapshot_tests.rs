use indoc::formatdoc;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};
use regex::Regex;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[test]
fn canister_snapshot() {
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
        .stderr(starts_with("Canister Status Report:").and(contains("Status: Running")));

    // List canister snapshots
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "list",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(starts_with("No snapshots found"));

    // Failed to create canister snapshot as the canister is running.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .failure()
        .stderr(starts_with("Error: Canister my-canister is running."));

    // Failed to load canister snapshot as the canister is running.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "load",
            "my-canister",
            "0000000000000003ffffffffffc000000101", // A faked snapshot id.
            "--environment",
            "my-environment",
        ])
        .assert()
        .failure()
        .stderr(starts_with("Error: Canister my-canister is running."));

    // Stop canister.
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

    // Create canister snapshot and parse the snapshot ID.
    let result = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    let result_str = std::str::from_utf8(&result.get_output().stderr).unwrap();
    assert!(result_str.starts_with("Created a new snapshot of canister"));

    let re = Regex::new(r"Snapshot ID: '([0-9a-fA-F]+)'").unwrap();
    let caps = re
        .captures(result_str)
        .expect("snapshot id not found in stderr");
    let snapshot_id = &caps[1];
    assert!(!snapshot_id.is_empty(), "snapshot id is empty");

    // List canister snapshots
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "list",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(contains(snapshot_id));

    // Load canister snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "load",
            "my-canister",
            snapshot_id,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(starts_with(format!("Loaded snapshot {}", snapshot_id)));

    // Delete canister snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "delete",
            "my-canister",
            snapshot_id,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(starts_with(format!("Deleted snapshot {}", snapshot_id)));

    // List canister snapshots
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "list",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success()
        .stderr(starts_with("No snapshots found"));
}
