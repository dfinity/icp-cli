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

/// Tests downloading a snapshot to disk and uploading it back
#[cfg(unix)] // moc
#[tokio::test]
async fn canister_snapshot_download_upload_roundtrip() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let snapshot_dir = ctx.create_project_dir("snapshot");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko"
              configuration:
                main: main.mo
                args: ""
            init_args: "(opt 42 : opt nat8)"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy canister with initial value 42
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

    // Verify initial value
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
        .stdout(contains("\"42\""));

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

    let original_snapshot_id = String::from_utf8_lossy(&create_output)
        .lines()
        .find(|line| line.contains("Created snapshot"))
        .and_then(|line| line.split_whitespace().nth(2))
        .expect("Could not extract snapshot ID")
        .to_string();

    // Download the snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "download",
            "my-canister",
            &original_snapshot_id,
            "--output",
            snapshot_dir.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Snapshot downloaded"));

    // Verify metadata file was created
    assert!(
        snapshot_dir.join("metadata.json").exists(),
        "metadata.json should exist"
    );

    // Delete the original snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "delete",
            "my-canister",
            &original_snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Upload the snapshot to create a new one
    let upload_output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "upload",
            "my-canister",
            "--input",
            snapshot_dir.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let uploaded_snapshot_id = String::from_utf8_lossy(&upload_output)
        .lines()
        .find(|line| line.contains("uploaded successfully"))
        .and_then(|line| line.split_whitespace().nth(1)) // "Snapshot <id> uploaded successfully"
        .expect("Could not extract uploaded snapshot ID")
        .to_string();

    // The uploaded snapshot should have a different ID
    assert_ne!(original_snapshot_id, uploaded_snapshot_id);

    // Reinstall canister with different value
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

    // Verify value changed
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

    // Stop and restore from uploaded snapshot
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

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "restore",
            "my-canister",
            &uploaded_snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Start and verify value is back to 42
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
        .stdout(contains("\"42\""));
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

/// Helper to generate large.wasm if it doesn't exist
fn ensure_large_wasm(ctx: &TestContext) -> PathBuf {
    let script_path = ctx.pkg_dir().join("tests/assets/generate_large_wasm.sh");
    let wasm_path = ctx.pkg_dir().join("tests/assets/large.wasm");

    if !wasm_path.exists() {
        std::process::Command::new("bash")
            .arg(&script_path)
            .current_dir(ctx.pkg_dir().join("tests/assets"))
            .status()
            .expect("failed to run generate_large_wasm.sh");
    }

    assert!(
        wasm_path.exists(),
        "large.wasm should exist after generation"
    );
    wasm_path
}

/// Helper to start mitmproxy as a reverse proxy
struct MitmproxyGuard {
    child: std::process::Child,
    port: u16,
}

impl MitmproxyGuard {
    /// Start mitmproxy allowing `limit_requests` request/response pairs through.
    /// Default of 2 allows metadata + one data chunk.
    fn start(target_port: u16, limit_requests: u32) -> Self {
        // Find a free port for mitmproxy
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_port = listener.local_addr().unwrap().port();
        drop(listener);

        let script_path = std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("tests/assets/limit_transfer.py");

        let child = std::process::Command::new("mitmdump")
            .args([
                "--mode",
                &format!("reverse:http://localhost:{target_port}"),
                "-p",
                &proxy_port.to_string(),
                "-s",
                script_path.as_str(),
                "--set",
                "flow_detail=0",
                "-q",
            ])
            .env("LIMIT_REQUESTS", limit_requests.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to start mitmproxy - is it installed?");

        // Give mitmproxy time to start
        std::thread::sleep(std::time::Duration::from_millis(500));

        Self {
            child,
            port: proxy_port,
        }
    }
}

impl Drop for MitmproxyGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Tests that download can resume after interruption
#[cfg(unix)]
#[tokio::test]
async fn canister_snapshot_download_resume() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let snapshot_dir = ctx.create_project_dir("snapshot");

    // Get the large wasm
    let large_wasm = ensure_large_wasm(&ctx);

    // Project manifest using prebuilt large.wasm
    let pm = formatdoc! {r#"
        canisters:
          - name: large-canister
            recipe:
              type: "@dfinity/prebuilt"
              configuration:
                path: "{wasm_path}"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#, wasm_path = large_wasm.as_str()};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Get the real network port from the descriptor
    let descriptor_bytes = ctx.read_network_descriptor(&project_dir, "random-network");
    let descriptor: serde_json::Value =
        serde_json::from_slice(&descriptor_bytes).expect("invalid descriptor JSON");
    let real_port = descriptor["gateway"]["port"].as_u64().unwrap() as u16;

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    // Deploy the large canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "large-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Stop and create snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "large-canister",
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
            "large-canister",
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

    // Start mitmproxy allowing 2 requests: metadata + one data chunk
    let proxy = MitmproxyGuard::start(real_port, 2);

    // Modify the network descriptor to route through mitmproxy
    let mut modified_descriptor = descriptor.clone();
    modified_descriptor["gateway"]["port"] = serde_json::json!(proxy.port);
    ctx.write_network_descriptor(
        &project_dir,
        "random-network",
        serde_json::to_vec_pretty(&modified_descriptor)
            .unwrap()
            .as_slice(),
    );

    // First download attempt should fail (proxy cuts off after 1 chunk)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "download",
            "large-canister",
            &snapshot_id,
            "--output",
            snapshot_dir.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure();

    // Verify partial download state exists
    assert!(
        snapshot_dir.join("metadata.json").exists(),
        "metadata.json should exist after partial download"
    );
    assert!(
        snapshot_dir.join(".download_progress.json").exists(),
        "download progress file should exist"
    );

    // Verify progress file shows intermediate state (some progress but not complete)
    let progress_content =
        std::fs::read_to_string(snapshot_dir.join(".download_progress.json")).unwrap();
    let progress: serde_json::Value = serde_json::from_str(&progress_content).unwrap();
    let frontier = progress["wasm_module"]["frontier"].as_u64().unwrap();
    let ahead_count = progress["wasm_module"]["ahead"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    // Wasm module is ~3MB, chunk size is 2MB. Intermediate state means exactly one chunk done.
    // Either frontier=2MB (first chunk done in order) or frontier=0 with one ahead chunk.
    let chunks_done = (frontier / 2_000_000) as usize + ahead_count;
    assert_eq!(
        chunks_done, 1,
        "exactly one chunk should have completed (frontier={frontier}, ahead={ahead_count})"
    );

    // Restore the real network descriptor for the resume
    ctx.write_network_descriptor(&project_dir, "random-network", &descriptor_bytes);

    // Resume download should succeed
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "download",
            "large-canister",
            &snapshot_id,
            "--output",
            snapshot_dir.as_str(),
            "--resume",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Snapshot downloaded"));

    // Progress file should be cleaned up
    assert!(
        !snapshot_dir.join(".download_progress.json").exists(),
        "download progress file should be cleaned up after success"
    );
}

/// Tests that upload can resume after interruption
#[cfg(unix)]
#[tokio::test]
async fn canister_snapshot_upload_resume() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let snapshot_dir = ctx.create_project_dir("snapshot");

    // Get the large wasm
    let large_wasm = ensure_large_wasm(&ctx);

    // Project manifest using prebuilt large.wasm
    let pm = formatdoc! {r#"
        canisters:
          - name: large-canister
            recipe:
              type: "@dfinity/prebuilt"
              configuration:
                path: "{wasm_path}"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#, wasm_path = large_wasm.as_str()};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Get the real network port from the descriptor
    let descriptor_bytes = ctx.read_network_descriptor(&project_dir, "random-network");
    let descriptor: serde_json::Value =
        serde_json::from_slice(&descriptor_bytes).expect("invalid descriptor JSON");
    let real_port = descriptor["gateway"]["port"].as_u64().unwrap() as u16;

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(100 * TRILLION);

    // Deploy the large canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "large-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Stop and create snapshot
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "large-canister",
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
            "large-canister",
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

    // Download the snapshot completely (without proxy interference)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "download",
            "large-canister",
            &snapshot_id,
            "--output",
            snapshot_dir.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Delete the snapshot so we can upload a new one
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "delete",
            "large-canister",
            &snapshot_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Start mitmproxy allowing 2 requests: metadata upload + one data chunk
    let proxy = MitmproxyGuard::start(real_port, 2);

    // Modify the network descriptor to route through mitmproxy
    let mut modified_descriptor = descriptor.clone();
    modified_descriptor["gateway"]["port"] = serde_json::json!(proxy.port);
    ctx.write_network_descriptor(
        &project_dir,
        "random-network",
        serde_json::to_vec_pretty(&modified_descriptor)
            .unwrap()
            .as_slice(),
    );

    // First upload attempt should fail
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "upload",
            "large-canister",
            "--input",
            snapshot_dir.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure();

    // Verify upload progress file exists
    assert!(
        snapshot_dir.join(".upload_progress.json").exists(),
        "upload progress file should exist after partial upload"
    );

    // Verify progress file shows intermediate state (some progress but not complete)
    let progress_content =
        std::fs::read_to_string(snapshot_dir.join(".upload_progress.json")).unwrap();
    let progress: serde_json::Value = serde_json::from_str(&progress_content).unwrap();
    let offset = progress["wasm_module_offset"].as_u64().unwrap();
    // Wasm module is ~3MB. Intermediate state means 0 < offset < 3MB.
    assert!(
        offset > 0 && offset < 3_000_000,
        "exactly one chunk should have been uploaded (offset={offset})"
    );

    // Restore the real network descriptor for the resume
    ctx.write_network_descriptor(&project_dir, "random-network", &descriptor_bytes);

    // Resume upload should succeed
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "snapshot",
            "upload",
            "large-canister",
            "--input",
            snapshot_dir.as_str(),
            "--resume",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("uploaded successfully"));

    // Progress file should be cleaned up
    assert!(
        !snapshot_dir.join(".upload_progress.json").exists(),
        "upload progress file should be cleaned up after success"
    );
}
