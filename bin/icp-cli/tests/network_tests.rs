use icp_fs::fs::write;
use icp_network::NETWORK_LOCAL;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};
use serial_test::file_serial;

mod common;
use crate::common::{TestContext, TestNetwork};

#[test]
fn network_random_port() {
    let ctx = TestContext::new().with_dfx();

    let project_dir = ctx.create_project_dir("icp");

    ctx.configure_icp_local_network_random_port(&project_dir);

    let _child_guard = ctx.start_network_in(&project_dir);

    // "icp network start" will wait for the local network to be healthy,
    // but for now we need to wait for the descriptor to be created.
    ctx.wait_for_local_network_descriptor(&project_dir);

    let test_network = ctx.configure_dfx_network(&project_dir, NETWORK_LOCAL);
    let dfx_network_name = test_network.dfx_network_name;
    let gateway_port = test_network.gateway_port;

    ctx.ping_until_healthy(&project_dir);

    ctx.dfx()
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

    let project_dir = ctx.home_path().join("hello");
    ctx.dfx()
        .current_dir(&project_dir)
        .arg("deploy")
        .arg("--no-wallet")
        .args(["--network", &dfx_network_name])
        .assert()
        .success();

    ctx.dfx()
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

    let output = ctx
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
    let ctx = TestContext::new().with_dfx();

    let project_dir_a = ctx.create_project_dir("a");
    let project_dir_b = ctx.create_project_dir("b");

    let _child_guard = ctx.start_network_in(&project_dir_a);

    eprintln!("wait for network healthy");
    ctx.ping_until_healthy(&project_dir_a);

    eprintln!("second network run attempt");
    ctx.icp()
        .current_dir(&project_dir_a)
        .args(["network", "run"])
        .assert()
        .failure()
        .stderr(contains(
            "the local network for this project is already running",
        ));

    eprintln!("second network run attempt in another project");
    ctx.icp()
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
    let ctx = TestContext::new().with_dfx();

    let project_dir_a = ctx.create_project_dir("a");
    let project_dir_b = ctx.create_project_dir("b");

    ctx.configure_icp_local_network_port(&project_dir_a, 8001);
    ctx.configure_icp_local_network_port(&project_dir_b, 8002);

    let _a_guard = ctx.start_network_in(&project_dir_a);

    eprintln!("wait for network A healthy");
    ctx.ping_until_healthy(&project_dir_a);

    let _b_guard = ctx.start_network_in(&project_dir_b);

    eprintln!("wait for network B healthy");
    ctx.ping_until_healthy(&project_dir_b);
}

#[test]
fn deploy_to_other_projects_network() {
    let ctx = TestContext::new().with_dfx();

    // Project A
    let proja = ctx.create_project_dir("project-a");
    ctx.configure_icp_local_network_random_port(&proja);

    // Start network
    let _g = ctx.start_network_in(&proja);

    let TestNetwork {
        gateway_port,
        root_key,
    } = ctx.wait_for_local_network_descriptor(&proja);

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project B
    let projb = ctx.create_project_dir("project-b");

    // Connect to Project A's network
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {wasm} "$ICP_WASM_OUTPUT_PATH"'

        networks:
          - name: network-a
            mode: connected
            url: http://localhost:{gateway_port}
            root-key: "{root_key}"

        environments:
          - name: environment-1
            network: network-a
        "#,
    );

    write(
        projb.join("icp.yaml"), // path
        pm,                     // contents
    )
    .expect("failed to write project manifest");

    ctx.ping_until_healthy(&proja);

    // Deploy project (first time)
    ctx.icp()
        .current_dir(&projb)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .args(["--environment", "environment-1"])
        .assert()
        .success();

    // Deploy project (second time)
    ctx.icp()
        .current_dir(&projb)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .args(["--environment", "environment-1"])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&projb)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .args(["--environment", "environment-1"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
