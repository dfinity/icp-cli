use icp::fs::write_string;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

mod common;
use crate::common::{NETWORK_RANDOM_PORT, TestContext};

#[test]
fn info_port_when_network_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Start network using CLI
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "start", "random-network", "--background"])
        .assert()
        .success()
        .stderr(contains("Installed Candid UI canister with ID"));

    let network = ctx.wait_for_network_descriptor(&project_dir, "random-network");

    // Test the info port command
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "info", "port", "random-network"])
        .assert()
        .success()
        .stdout(eq(format!("{}\n", network.gateway_port)));

    // Stop network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "random-network"])
        .assert()
        .success();
}

#[test]
fn info_port_fixed_port() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

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
        .assert()
        .success();

    ctx.wait_for_network_descriptor(&project_dir, "fixed-network");

    // Test the info port command returns the fixed port
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "info", "port", "fixed-network"])
        .assert()
        .success()
        .stdout(eq("8123\n"));

    // Stop network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "fixed-network"])
        .assert()
        .success();
}

#[test]
fn info_candid_ui_principal_when_network_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Start network using CLI
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "start", "random-network", "--background"])
        .assert()
        .success()
        .stderr(contains("Installed Candid UI canister with ID"));

    ctx.wait_for_network_descriptor(&project_dir, "random-network");
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Test the info candid-ui-principal command
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "info", "candid-ui-principal", "random-network"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let principal_str = String::from_utf8(output).expect("output should be valid UTF-8");
    let principal_str = principal_str.trim();

    // Verify it's a valid principal (should be parseable)
    candid::Principal::from_text(principal_str).expect("output should be a valid principal");

    // Stop network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "random-network"])
        .assert()
        .success();
}

#[test]
fn info_port_when_network_not_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Don't start the network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "info", "port", "random-network"])
        .assert()
        .failure()
        .stderr(contains("network 'random-network' is not running"));
}

#[test]
fn info_candid_ui_principal_when_network_not_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Don't start the network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "info", "candid-ui-principal", "random-network"])
        .assert()
        .failure()
        .stderr(contains("network 'random-network' is not running"));
}

#[test]
fn info_port_nonexistent_network() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "info", "port", "nonexistent"])
        .assert()
        .failure()
        .stderr(contains(
            "project does not contain a network named 'nonexistent'",
        ));
}

#[test]
fn info_candid_ui_principal_nonexistent_network() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "info", "candid-ui-principal", "nonexistent"])
        .assert()
        .failure()
        .stderr(contains(
            "project does not contain a network named 'nonexistent'",
        ));
}

#[test]
fn info_connected_network_fails() {
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
        .args(["network", "info", "port", "connected-network"])
        .assert()
        .failure()
        .stderr(contains(
            "network 'connected-network' is not a managed network",
        ));
}

#[test]
fn info_not_in_project() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "info", "port"])
        .assert()
        .failure()
        .stderr(contains("Error: failed to locate project directory").trim());
}

#[test]
fn info_help() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "info", "--help"])
        .assert()
        .success()
        .stdout(contains("Get information about a running network"))
        .stdout(contains("port"))
        .stdout(contains("candid-ui-principal"));
}
