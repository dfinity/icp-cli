mod common;

use crate::common::TestEnv;
use icp_fs::fs::write;
use predicates::str::contains;
use predicates::{ord::eq, str::PredicateStrExt};
use serial_test::file_serial;

#[test]
#[file_serial(default_local_network)]
fn hello() {
    let testenv = TestEnv::new().with_dfx();

    let icp_project_dir = testenv.create_project_dir("icp");

    let _child_guard = testenv.start_network_in(&icp_project_dir);

    testenv.configure_dfx_local_network();

    testenv.ping_until_healthy(&icp_project_dir);

    testenv
        .dfx()
        .arg("new")
        .arg("hello")
        .arg("--type")
        .arg("motoko")
        .arg("--frontend")
        .arg("simple-assets")
        .arg("--agent-version") // don't contact npm to look up the agent-js version
        .arg("99.99")
        .assert()
        .success();

    let project_dir = testenv.home_path().join("hello");
    testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("deploy")
        .arg("--no-wallet")
        .assert()
        .success();

    testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("canister")
        .arg("call")
        .arg("hello_backend")
        .arg("greet")
        .arg(r#"("test")"#)
        .assert()
        .success()
        .stdout(contains(r#"("Hello, test!")"#));

    let output = testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("canister")
        .arg("id")
        .arg("hello_frontend")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let frontend_canister_id = std::str::from_utf8(&output)
        .expect("stdout was not valid UTF-8")
        .trim();

    let url = format!("http://localhost:8000/sample-asset.txt?canisterId={frontend_canister_id}");
    let response = reqwest::blocking::get(&url)
        .expect("Failed to fetch static asset")
        .text()
        .expect("Failed to read response text");
    assert_eq!(
        response, "This is a sample asset!\n",
        "Static asset content mismatch"
    );
}

#[test]
fn network_random_port() {
    let testenv = TestEnv::new().with_dfx();

    let project_dir = testenv.create_project_dir("icp");

    testenv.configure_icp_local_network_random_port(&project_dir);

    let _child_guard = testenv.start_network_in(&project_dir);

    // "icp network start" will wait for the local network to be healthy,
    // but for now we need to wait for the descriptor to be created.
    testenv.wait_for_local_network_descriptor(&project_dir);

    let test_network = testenv.configure_dfx_network(&project_dir, "local");
    let dfx_network_name = test_network.dfx_network_name;
    let gateway_port = test_network.gateway_port;

    testenv.ping_until_healthy(&project_dir);

    testenv
        .dfx()
        .arg("new")
        .arg("hello")
        .arg("--type")
        .arg("motoko")
        .arg("--frontend")
        .arg("simple-assets")
        .arg("--agent-version") // don't contact npm to look up the agent-js version
        .arg("99.99")
        .assert()
        .success();

    let project_dir = testenv.home_path().join("hello");
    testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("deploy")
        .arg("--no-wallet")
        .args(["--network", &dfx_network_name])
        .assert()
        .success();

    testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("canister")
        .arg("call")
        .arg("hello_backend")
        .arg("greet")
        .arg(r#"("test")"#)
        .args(["--network", &dfx_network_name])
        .assert()
        .success()
        .stdout(contains(r#"("Hello, test!")"#));

    let output = testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("canister")
        .arg("id")
        .arg("hello_frontend")
        .args(["--network", &dfx_network_name])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let frontend_canister_id = std::str::from_utf8(&output)
        .expect("stdout was not valid UTF-8")
        .trim();

    let url = format!(
        "http://localhost:{gateway_port}/sample-asset.txt?canisterId={frontend_canister_id}"
    );
    let response = reqwest::blocking::get(&url)
        .expect("Failed to fetch static asset")
        .text()
        .expect("Failed to read response text");
    assert_eq!(
        response, "This is a sample asset!\n",
        "Static asset content mismatch"
    );
}

#[test]
#[file_serial(default_local_network)]
fn network_same_port() {
    let testenv = TestEnv::new().with_dfx();

    let project_dir_a = testenv.create_project_dir("a");
    let project_dir_b = testenv.create_project_dir("b");

    let _child_guard = testenv.start_network_in(&project_dir_a);

    eprintln!("wait for network healthy");
    testenv.ping_until_healthy(&project_dir_a);

    eprintln!("second network run attempt");
    testenv
        .icp()
        .current_dir(&project_dir_a)
        .args(["network", "run"])
        .assert()
        .failure()
        .stderr(contains(
            "the local network for this project is already running",
        ));

    eprintln!("second network run attempt in another project");
    testenv
        .icp()
        .current_dir(&project_dir_b)
        .args(["network", "run"])
        .assert()
        .failure()
        .stderr(contains(
            "port 8000 is in use by the local network of the project at",
        ));
}

#[test]
#[file_serial(port8001, port8002)]
fn two_projects_different_fixed_ports() {
    let testenv = TestEnv::new().with_dfx();

    let project_dir_a = testenv.create_project_dir("a");
    let project_dir_b = testenv.create_project_dir("b");

    testenv.configure_icp_local_network_port(&project_dir_a, 8001);
    testenv.configure_icp_local_network_port(&project_dir_b, 8002);

    let _a_guard = testenv.start_network_in(&project_dir_a);

    eprintln!("wait for network A healthy");
    testenv.ping_until_healthy(&project_dir_a);

    let _b_guard = testenv.start_network_in(&project_dir_b);

    eprintln!("wait for network B healthy");
    testenv.ping_until_healthy(&project_dir_b);
}

#[test]
fn deploy_to_other_projects_network() {
    let env = TestEnv::new().with_dfx();

    // Setup project that runs a network
    let project_dir_a = env.create_project_dir("icp-a");
    env.configure_icp_local_network_random_port(&project_dir_a);
    let _g = env.start_network_in(&project_dir_a);
    let test_network = env.wait_for_local_network_descriptor(&project_dir_a);

    let project_dir_b = env.create_project_dir("icp-b");
    let project_dir_b_networks = project_dir_b.join("networks");
    std::fs::create_dir_all(&project_dir_b_networks)
        .expect("Failed to create networks directory for project B");

    // Configure a network for project B to use the project A's network
    let network_config = format!(
        r#"
        mode: connected
        url: http://localhost:{}
        root-key: "{}"
        "#,
        test_network.gateway_port, test_network.root_key,
    );
    std::fs::write(
        project_dir_b_networks.join("project-a.yaml"),
        network_config,
    )
    .expect("Failed to write network config for project B");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        wasm,
    );

    write(
        project_dir_b.join("icp.yaml"), // path
        pm,                             // contents
    )
    .expect("failed to write project manifest");

    env.ping_until_healthy(&project_dir_a);

    // Deploy project (first time)
    env.icp()
        .current_dir(&project_dir_b)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .args(["--network", "project-a"])
        .assert()
        .success();

    // Deploy project (second time)
    env.icp()
        .current_dir(&project_dir_b)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .args(["--network", "project-a"])
        .assert()
        .success();

    // Query canister
    env.icp()
        .current_dir(&project_dir_b)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .args(["--network", "project-a"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[test]
fn round_robin_routing() {
    let env = TestEnv::new().with_dfx();

    // Setup project that runs a network
    let project_dir_a = env.create_project_dir("icp-a");
    env.configure_icp_local_network_random_port(&project_dir_a);
    let _g = env.start_network_in(&project_dir_a);
    let test_network = env.wait_for_local_network_descriptor(&project_dir_a);

    let project_dir_b = env.create_project_dir("icp-b");
    let project_dir_b_networks = project_dir_b.join("networks");
    std::fs::create_dir_all(&project_dir_b_networks)
        .expect("Failed to create networks directory for project B");

    // Configure a network for project B to use the project A's network
    let network_config = format!(
        r#"
        mode: connected
        urls:
          - http://localhost:{}
          - http://127.0.0.1:{}
        root-key: "{}"
        "#,
        test_network.gateway_port, test_network.gateway_port, test_network.root_key,
    );
    std::fs::write(
        project_dir_b_networks.join("project-a.yaml"),
        network_config,
    )
    .expect("Failed to write network config for project B");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        wasm,
    );

    write(
        project_dir_b.join("icp.yaml"), // path
        pm,                             // contents
    )
    .expect("failed to write project manifest");

    env.ping_until_healthy(&project_dir_a);

    // Deploy project (first time)
    env.icp()
        .current_dir(&project_dir_b)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .args(["--network", "project-a"])
        .assert()
        .success();

    // Deploy project (second time)
    env.icp()
        .current_dir(&project_dir_b)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .args(["--network", "project-a"])
        .assert()
        .success();

    // Query canister
    env.icp()
        .current_dir(&project_dir_b)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .args(["--network", "project-a"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
