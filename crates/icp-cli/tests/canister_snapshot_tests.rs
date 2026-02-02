use indoc::formatdoc;
use predicates::str::contains;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

/// Tests the full snapshot workflow: create -> list -> restore -> delete -> list
#[cfg(unix)] // moc
#[tokio::test]
async fn canister_snapshot_workflow() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Project manifest with Motoko recipe
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko"
              configuration:
                main: main.mo
                args: ""
            init_args: "(opt 1 : opt nat8)"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy canister with initial value 1
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify initial value is 1
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(contains("\"1\""));

    // Stop the canister before creating snapshot
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

    // Create a snapshot
    let create_output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let create_output_str = String::from_utf8_lossy(&create_output);
    assert!(
        create_output_str.contains("Created snapshot"),
        "Expected 'Created snapshot' in output, got: {}",
        create_output_str
    );

    // Extract snapshot ID from output (it's a hex string after "Created snapshot ")
    let snapshot_id = create_output_str
        .lines()
        .find(|line| line.contains("Created snapshot"))
        .and_then(|line| line.split_whitespace().nth(2))
        .expect("Could not extract snapshot ID from output");

    // List snapshots - should show the one we created
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "list",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains(snapshot_id));

    // Start the canister again before reinstalling
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "start",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Reinstall canister with new value 99
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "my-canister",
            "--environment",
            "random-environment",
            "--mode",
            "reinstall",
            "--args",
            "(opt 99 : opt nat8)",
        ])
        .assert()
        .success();

    // Verify value is now 99
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(contains("\"99\""));

    // Stop the canister before restoring
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

    // Restore from snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "restore",
            "my-canister",
            snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Restored canister"));

    // Start the canister again
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "start",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify value is back to 1 (from snapshot)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(contains("\"1\""));

    // Delete the snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "delete",
            "my-canister",
            snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Deleted snapshot"));

    // List snapshots - should be empty now
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "list",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("No snapshots found"));
}

/// Tests creating a snapshot with the --replace flag
#[cfg(unix)] // moc
#[tokio::test]
async fn canister_snapshot_replace() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko"
              configuration:
                main: main.mo
                args: ""
            init_args: "(opt 10 : opt nat8)"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Stop the canister before creating snapshots
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

    // Create first snapshot
    let create_output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let first_snapshot_id = String::from_utf8_lossy(&create_output)
        .lines()
        .find(|line| line.contains("Created snapshot"))
        .and_then(|line| line.split_whitespace().nth(2))
        .expect("Could not extract snapshot ID")
        .to_string();

    // Create second snapshot replacing the first (canister already stopped)
    let create_output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--replace",
            &first_snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let second_snapshot_id = String::from_utf8_lossy(&create_output)
        .lines()
        .find(|line| line.contains("Created snapshot"))
        .and_then(|line| line.split_whitespace().nth(2))
        .expect("Could not extract snapshot ID")
        .to_string();

    // The snapshot IDs should be different
    assert_ne!(first_snapshot_id, second_snapshot_id);

    // List should only show the second snapshot (first was replaced)
    let list_output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "list",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let list_output_str = String::from_utf8_lossy(&list_output);
    assert!(
        list_output_str.contains(&second_snapshot_id),
        "Expected second snapshot ID in list"
    );
    assert!(
        !list_output_str.contains(&first_snapshot_id),
        "First snapshot should have been replaced"
    );
}

/// Tests that running canisters cannot be snapshotted or restored
#[cfg(unix)] // moc
#[tokio::test]
async fn canister_snapshot_requires_stopped() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko"
              configuration:
                main: main.mo
                args: ""
            init_args: "(opt 1 : opt nat8)"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Attempt to create snapshot on running canister - should fail
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("currently running"))
        .stderr(contains("icp canister stop"));

    // Stop, create snapshot, then start the canister
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

    let create_output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot_id = String::from_utf8_lossy(&create_output)
        .lines()
        .find(|line| line.contains("Created snapshot"))
        .and_then(|line| line.split_whitespace().nth(2))
        .expect("Could not extract snapshot ID")
        .to_string();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "start",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Attempt to restore snapshot on running canister - should fail
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "restore",
            "my-canister",
            &snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("currently running"))
        .stderr(contains("icp canister stop"));
}
