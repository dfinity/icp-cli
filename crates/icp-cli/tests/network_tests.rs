use icp::{fs::write_string, prelude::*};
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};
use serial_test::file_serial;

mod common;
use crate::common::{TestContext, TestNetwork, clients};

#[test]
#[file_serial(default_local_network)]
fn network_same_port() {
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

    // Project A
    let proja = ctx.create_project_dir("project-a");
    ctx.configure_icp_local_network_random_port(&proja);

    // Start network
    let _g = ctx.start_network_in(&proja);

    let TestNetwork {
        gateway_port,
        root_key,
        ..
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

    write_string(
        &projb.join("icp.yaml"), // path
        &pm,                     // contents
    )
    .expect("failed to write project manifest");

    ctx.ping_until_healthy(&proja);

    // Deploy project (first time)
    clients::icp(&ctx, &proja).mint_cycles(10 * TRILLION);

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

#[test]
fn network_seeds_preexisting_identities_icp_and_cycles_balances() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");
    let icp_client = clients::icp(&ctx, &project_dir);

    // Create identities BEFORE starting the network
    icp_client.create_identity("before");

    // Time how long it takes to configure and start the network
    let start = std::time::Instant::now();
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _guard = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);
    let duration = start.elapsed();
    println!(
        "========== Configuring and starting network took {:?}",
        duration
    );

    // Create identities AFTER starting the network
    icp_client.create_identity("after");

    // Anonymouys starts with massive initial balance
    icp_client.use_identity("anonymous");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance"])
        .assert()
        .stdout(contains("Balance: 1000000000.00000000 ICP"))
        .success();

    // Identities created before starting should have a large seeded ICP balance
    icp_client.use_identity("before");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance"])
        .assert()
        .stdout(contains("Balance: 1000000.00000000 ICP"))
        .success();

    // Identities created after starting should have 0 ICP balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance"])
        .assert()
        .stdout(contains("Balance: 0 ICP"))
        .success();

    // Identities created before starting should have a large seeded cycles balance
    icp_client.use_identity("before");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 1000.000000000000 TCYCLES"))
        .success();

    // Identities created after starting should have 0 cycles balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 0 TCYCLES"))
        .success();
}
