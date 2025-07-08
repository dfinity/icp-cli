mod common;

use crate::common::TestEnv;
use crate::common::predicates::json::field::json_field;
use assert_cmd::assert::Assert;
use camino::Utf8Path;
use predicates::str::contains;
use predicates::{ord::eq, str::PredicateStrExt};
use serde_json::Value;
use serial_test::file_serial;

#[test]
fn ping_local() {
    let testenv = TestEnv::new();

    let project_dir = testenv.create_project_dir("icp");
    testenv.configure_icp_local_network_random_port(&project_dir);

    let _child_guard = testenv.start_network_in(&project_dir);

    let network_descriptor = testenv.wait_for_local_network_descriptor(&project_dir);
    let expected_root_key = hex::decode(&network_descriptor.root_key)
        .expect("Failed to decode root key from hex")
        .into_iter()
        .map(|byte| Value::Number(serde_json::Number::from(byte)))
        .collect::<Vec<Value>>();

    testenv
        .icp()
        .current_dir(&project_dir)
        .args(["network", "ping"])
        .assert()
        .success()
        .stdout(json_field("root_key").array().equals(expected_root_key))
        .stdout(
            json_field("replica_health_status")
                .string()
                .equals("healthy"),
        );
}

#[test]
fn ping_cli_options() {
    // Test that the `--network <network>` and `ICP_NETWORK` options work as expected
    // in addition to the positional argument.

    let testenv = TestEnv::new();

    let project_dir_a = testenv.create_project_dir("project-a");
    testenv.configure_icp_local_network_random_port(&project_dir_a);
    let mut child_guard_a = Some(testenv.start_network_in(&project_dir_a));
    let network_descriptor_a = testenv.wait_for_local_network_descriptor(&project_dir_a);

    let project_dir_b = testenv.create_project_dir("project-b");
    testenv.configure_icp_local_network_random_port(&project_dir_b);
    let _child_guard_b = Some(testenv.start_network_in(&project_dir_b));
    let network_descriptor_b = testenv.wait_for_local_network_descriptor(&project_dir_b);

    let no_network_project_dir = testenv.create_project_dir("icp");
    let networks_dir = no_network_project_dir.join("networks");
    std::fs::create_dir_all(&networks_dir).unwrap();

    std::fs::write(
        networks_dir.join("project-a.yaml"),
        format!(
            r#"
        mode: connected
        url: http://localhost:{}
        "#,
            network_descriptor_a.gateway_port
        ),
    )
    .unwrap();

    std::fs::write(
        networks_dir.join("project-b.yaml"),
        format!(
            r#"
        mode: connected
        url: http://localhost:{}
        "#,
            network_descriptor_b.gateway_port
        ),
    )
    .unwrap();

    enum NetworkArg<'a> {
        DefaultLocal,
        Positional(&'a str),
        Flag(&'a str),
        Environment(&'a str),
    }
    let assert_ping = |dir: &Utf8Path, param_type: NetworkArg| -> Assert {
        let mut cmd = testenv.icp();
        let cmd = cmd.current_dir(dir).args(["network", "ping"]);
        let cmd = match param_type {
            NetworkArg::DefaultLocal => cmd,
            NetworkArg::Positional(network) => cmd.arg(network),
            NetworkArg::Flag(network) => cmd.arg("--network").arg(network),
            NetworkArg::Environment(network) => cmd.env("ICP_NETWORK", network),
        };
        cmd.assert()
    };

    // each can ping their own network
    assert_ping(&project_dir_a, NetworkArg::DefaultLocal).success();
    assert_ping(&project_dir_b, NetworkArg::DefaultLocal).success();

    // the project with no network cannot ping its own network
    assert_ping(&no_network_project_dir, NetworkArg::DefaultLocal)
        .failure()
        .stderr(eq("Error: the local network for this project is not running").trim());

    let project_a = "project-a";
    let project_b = "project-b";

    // from a project not running a network, we can ping project-a and project-b
    assert_ping(&no_network_project_dir, NetworkArg::Positional(project_a)).success();
    assert_ping(&no_network_project_dir, NetworkArg::Flag(project_a)).success();
    assert_ping(&no_network_project_dir, NetworkArg::Environment(project_a)).success();

    assert_ping(&no_network_project_dir, NetworkArg::Positional(project_b)).success();
    assert_ping(&no_network_project_dir, NetworkArg::Flag(project_b)).success();
    assert_ping(&no_network_project_dir, NetworkArg::Environment(project_b)).success();

    // PocketIc seems to use an identical root key for all local networks,
    // so we'll stop one network and make sure we can still ping the other one.
    child_guard_a.take().unwrap();

    // can't ping project-a anymore
    assert_ping(&no_network_project_dir, NetworkArg::Positional(project_a))
        .failure()
        .stderr(contains("failed to ping the network"));

    assert_ping(&no_network_project_dir, NetworkArg::Flag(project_a))
        .failure()
        .stderr(contains("failed to ping the network"));

    assert_ping(&no_network_project_dir, NetworkArg::Environment(project_a))
        .failure()
        .stderr(contains("failed to ping the network"));

    // can still ping project-b
    assert_ping(&no_network_project_dir, NetworkArg::Positional(project_b)).success();
    assert_ping(&no_network_project_dir, NetworkArg::Flag(project_b)).success();
    assert_ping(&no_network_project_dir, NetworkArg::Environment(project_b)).success();
}

#[file_serial(default_local_network)]
#[test]
fn attempt_ping_other_project() {
    let testenv = TestEnv::new();

    let project_dir_a = testenv.create_project_dir("a");

    // Start project a's local network, then stop it, but write the
    // network descriptor as if the network were killed
    let network_descriptor = {
        let _g = testenv.start_network_in(&project_dir_a);

        // load network descriptor
        let network_descriptor = testenv.read_network_descriptor(&project_dir_a, "local");
        eprintln!(
            "Network descriptor for project 'a': {}",
            serde_json::to_string_pretty(&network_descriptor).unwrap()
        );

        network_descriptor
    };

    testenv.write_network_descriptor(&project_dir_a, "local", &network_descriptor);

    let project_dir_b = testenv.create_project_dir("b");

    let _child_guard_b = testenv.start_network_in(&project_dir_b);

    let network_descriptor = testenv.wait_for_local_network_descriptor(&project_dir_b);
    let expected_root_key = hex::decode(&network_descriptor.root_key)
        .expect("Failed to decode root key from hex")
        .into_iter()
        .map(|byte| Value::Number(serde_json::Number::from(byte)))
        .collect::<Vec<Value>>();

    testenv
        .icp()
        .current_dir(&project_dir_b)
        .args(["network", "ping"])
        .assert()
        .success()
        .stdout(
            json_field("root_key")
                .array()
                .equals(expected_root_key.clone()),
        )
        .stdout(
            json_field("replica_health_status")
                .string()
                .equals("healthy"),
        );

    let expected = format!(
        r#"Error: port 8000 is already in use by the local network of another project at {}"#,
        project_dir_b.canonicalize().unwrap().display()
    );

    testenv
        .icp()
        .current_dir(&project_dir_a)
        .args(["network", "ping"])
        .assert()
        .failure()
        .stderr(eq(expected).trim());
}

#[test]
fn ping_not_running() {
    let testenv = TestEnv::new().with_dfx();

    let icp_project_dir = testenv.create_project_dir("icp");

    testenv
        .icp()
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
    let testenv = TestEnv::new();

    testenv
        .icp()
        .args(["network", "ping"])
        .assert()
        .failure()
        .stderr(
            eq("Error: no project (icp.yaml) found in current directory or its parents").trim(),
        );
}
