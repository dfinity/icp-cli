use icp::fs::write_string;
use predicates::str::{PredicateStrExt, contains};
use serde_json::Value;

mod common;
use crate::common::{NETWORK_RANDOM_PORT, TestContext};

#[tokio::test]
async fn ping_network() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(
        &project_dir.join("icp.yaml"), // path
        NETWORK_RANDOM_PORT,           // contents
    )
    .expect("failed to write project manifest");

    let _child_guard = ctx.start_network_in(&project_dir, "random-network").await;

    let network_descriptor = ctx.wait_for_network_descriptor(&project_dir, "random-network");
    let expected_root_key = network_descriptor
        .root_key
        .into_iter()
        .map(|byte| Value::Number(serde_json::Number::from(byte)))
        .collect::<Vec<Value>>();

    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "ping", "random-network"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).expect("stdout was not valid JSON");

    let root_key = json
        .get("root_key")
        .expect("missing 'root_key' field")
        .as_array()
        .expect("'root_key' was not an array");

    assert_eq!(
        root_key, &expected_root_key,
        "unexpected value for 'root_key'"
    );

    let status = json
        .get("replica_health_status")
        .expect("missing 'replica_health_status' field")
        .as_str()
        .expect("'replica_health_status' was not a string");

    assert_eq!(status, "healthy", "unexpected replica_health_status");
}

#[test]
fn ping_not_running() {
    let ctx = TestContext::new();

    let icp_project_dir = ctx.create_project_dir("icp");

    ctx.icp()
        .current_dir(&icp_project_dir)
        .args(["network", "ping"])
        .assert()
        .failure()
        .stderr(contains(
            "the local network for this project is not running",
        ));
}

#[test]
fn ping_not_in_project() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "ping"])
        .assert()
        .failure()
        .stderr(contains("Error: failed to locate project directory").trim());
}
