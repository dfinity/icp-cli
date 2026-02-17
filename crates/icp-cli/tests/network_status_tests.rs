use icp::fs::write_string;
use predicates::str::{PredicateStrExt, contains};

mod common;
use crate::common::{NETWORK_RANDOM_PORT, TestContext};

#[tokio::test]
async fn status_when_network_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let launcher_path = ctx.launcher_path_or_nothing().await;

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Start network using CLI
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "start", "random-network", "--background"])
        .env("ICP_CLI_NETWORK_LAUNCHER_PATH", &launcher_path)
        .assert()
        .success()
        .stderr(contains("Network started on port"));

    ctx.wait_for_network_descriptor(&project_dir, "random-network");

    // Test the status command shows all fields
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network"])
        .assert()
        .success()
        .stdout(contains("Url:"))
        .stdout(contains("Root Key:"))
        .stdout(contains("Candid UI Principal:"))
        .stdout(contains("Proxy Canister Principal:"));

    // Stop network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "random-network"])
        .assert()
        .success();
}

#[tokio::test]
async fn status_with_json() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let launcher_path = ctx.launcher_path_or_nothing().await;

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Start network using CLI
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "start", "random-network", "--background"])
        .env("ICP_CLI_NETWORK_LAUNCHER_PATH", &launcher_path)
        .assert()
        .success()
        .stderr(contains("Network started on port"));

    ctx.wait_for_network_descriptor(&project_dir, "random-network");

    // Test the status command with JSON output
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json_str = String::from_utf8(output).expect("output should be valid UTF-8");
    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    // Verify JSON structure
    assert!(json.get("api_url").is_some());
    assert!(json.get("root_key").is_some());
    assert!(json.get("candid_ui_principal").is_some());
    assert!(json.get("proxy_canister_principal").is_some());

    // Stop network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "random-network"])
        .assert()
        .success();
}

#[tokio::test]
async fn status_fixed_port() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let launcher_path = ctx.launcher_path_or_nothing().await;

    // Project manifest with fixed port
    write_string(
        &project_dir.join("icp.yaml"),
        r#"
networks:
  - name: fixed-network
    mode: managed
    gateway:
      port: 8123
"#,
    )
    .expect("failed to write project manifest");

    // Start network using CLI
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "start", "fixed-network", "--background"])
        .env("ICP_CLI_NETWORK_LAUNCHER_PATH", &launcher_path)
        .assert()
        .success();

    ctx.wait_for_network_descriptor(&project_dir, "fixed-network");

    // Test the status command shows the fixed port
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "fixed-network"])
        .assert()
        .success()
        .stdout(contains("Url: http://localhost:8123"));

    // Stop network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "fixed-network"])
        .assert()
        .success();
}

#[test]
fn status_when_network_not_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Don't start the network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network"])
        .assert()
        .failure()
        .stderr(contains(
            "unable to access network 'random-network', is it running",
        ));
}

#[test]
fn status_nonexistent_network() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "nonexistent"])
        .assert()
        .failure()
        .stderr(contains(
            "project does not contain a network named 'nonexistent'",
        ));
}

#[test]
fn status_connected_network() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with connected network
    write_string(
        &project_dir.join("icp.yaml"),
        r#"
networks:
  - name: connected-network
    mode: connected
    url: https://ic0.app
"#,
    )
    .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "connected-network"])
        .assert()
        .success()
        .stdout(contains("Url: https://ic0.app"));
}

#[test]
fn status_not_in_project() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "status"])
        .assert()
        .failure()
        .stderr(contains("Error: failed to locate project directory").trim());
}

#[test]
fn status_help() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "status", "--help"])
        .assert()
        .success()
        .stdout(contains("Get status information about a running network"));
}
