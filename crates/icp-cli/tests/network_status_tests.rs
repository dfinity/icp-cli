use icp::fs::write_string;
use predicates::str::{PredicateStrExt, contains};

mod common;
use crate::common::{NETWORK_RANDOM_PORT, TestContext};

#[tokio::test]
async fn status_when_network_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    let _guard = ctx.start_network_in(&project_dir, "random-network").await;

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network"])
        .assert()
        .success()
        .stdout(contains("Url:"))
        .stdout(contains("Root Key:"))
        .stdout(contains("Candid UI Principal:"))
        .stdout(contains("Proxy Canister Principal:"));
}

#[tokio::test]
async fn status_with_json() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    let _guard = ctx.start_network_in(&project_dir, "random-network").await;

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
}

#[tokio::test]
async fn status_fixed_port() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

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

    let _guard = ctx.start_network_in(&project_dir, "fixed-network").await;

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "fixed-network"])
        .assert()
        .success()
        .stdout(contains("Url: http://localhost:8123"));
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

    // Project manifest with a connected network. A remote network must specify
    // its root key explicitly (only a local/loopback network has its key
    // fetched automatically).
    write_string(
        &project_dir.join("icp.yaml"),
        r#"
networks:
  - name: connected-network
    mode: connected
    url: https://ic0.app
    root-key: 308182301d060d2b0601040182dc7c0503010201060c2b0601040182dc7c05030201036100814c0e6ec71fab583b08bd81373c255c3c371b2e84863c98a4f1e08b74235d14fb5d9c0cd546d9685f913a0c0b2cc5341583bf4b4392e467db96d65b9bb4cb717112f8472e0d5a4d14505ffd7484b01291091c5f87b98883463f98091a0baaae
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

/// A connected network at a remote URL must specify its root key; accessing one
/// without a key fails fast rather than silently assuming the mainnet key.
#[test]
fn status_connected_network_remote_without_root_key_errors() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

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
        .failure()
        .stderr(contains(
            "a root key is required to connect to remote network",
        ));
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
