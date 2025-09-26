use icp_network::NETWORK_LOCAL;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};
use serde_json::Value;
use serial_test::file_serial;

mod common;
use crate::common::TestContext;

#[test]
fn ping_local() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");
    ctx.configure_icp_local_network_random_port(&project_dir);

    let _child_guard = ctx.start_network_in(&project_dir);

    let network_descriptor = ctx.wait_for_local_network_descriptor(&project_dir);
    let expected_root_key = hex::decode(&network_descriptor.root_key)
        .expect("Failed to decode root key from hex")
        .into_iter()
        .map(|byte| Value::Number(serde_json::Number::from(byte)))
        .collect::<Vec<Value>>();

    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "ping"])
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

#[file_serial(default_local_network)]
#[test]
fn attempt_ping_other_project() {
    let ctx = TestContext::new();

    let project_dir_a = ctx.create_project_dir("a");

    // Start project a's local network, then stop it, but write the
    // network descriptor as if the network were killed
    let network_descriptor = {
        let _g = ctx.start_network_in(&project_dir_a);

        // load network descriptor
        let network_descriptor = ctx.read_network_descriptor(&project_dir_a, NETWORK_LOCAL);
        eprintln!(
            "Network descriptor for project 'a': {}",
            serde_json::to_string_pretty(&network_descriptor).unwrap()
        );

        network_descriptor
    };

    ctx.write_network_descriptor(&project_dir_a, NETWORK_LOCAL, &network_descriptor);

    let project_dir_b = ctx.create_project_dir("b");

    let _child_guard_b = ctx.start_network_in(&project_dir_b);

    let network_descriptor = ctx.wait_for_local_network_descriptor(&project_dir_b);
    let expected_root_key = hex::decode(&network_descriptor.root_key)
        .expect("Failed to decode root key from hex")
        .into_iter()
        .map(|byte| Value::Number(serde_json::Number::from(byte)))
        .collect::<Vec<Value>>();

    let output = ctx
        .icp()
        .current_dir(&project_dir_b)
        .args(["network", "ping"])
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

    let expected = format!(
        r#"Error: port 8000 is already in use by the local network of another project at {}"#,
        project_dir_b.canonicalize().unwrap().display()
    );

    ctx.icp()
        .current_dir(&project_dir_a)
        .args(["network", "ping"])
        .assert()
        .failure()
        .stderr(eq(expected).trim());
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
        .stderr(
            eq("Error: no project (icp.yaml) found in current directory or its parents").trim(),
        );
}
