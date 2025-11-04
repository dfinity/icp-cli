use icp_canister_interfaces::{
    cycles_ledger::CYCLES_LEDGER_PRINCIPAL,
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL, icp_ledger::ICP_LEDGER_PRINCIPAL,
    internet_identity::INTERNET_IDENTITY_PRINCIPAL, nns_root::NNS_ROOT_PRINCIPAL,
    registry::REGISTRY_PRINCIPAL,
};
use indoc::{formatdoc, indoc};
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};
use serial_test::file_serial;
use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::common::{
    ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, TestNetwork, clients,
};
use icp::{
    fs::{read_to_string, write_string},
    prelude::*,
};

mod common;

#[test]
#[file_serial(default_local_network)]
fn network_same_port() {
    let ctx = TestContext::new();

    let project_dir_a = ctx.create_project_dir("a");

    // Project manifest
    let pm = indoc! {r#"
        networks:
          - name: my-network
            mode: managed
            gateway:
              port: 8080
    "#};

    // write manifest to project a
    write_string(
        &project_dir_a.join("icp.yaml"), // path
        pm,
    )
    .expect("failed to write project manifest");

    let project_dir_b = ctx.create_project_dir("b");

    // write manifest to project b
    write_string(
        &project_dir_b.join("icp.yaml"), // path
        pm,
    )
    .expect("failed to write project manifest");

    let _a_guard = ctx.start_network_in(&project_dir_a, "my-network");

    eprintln!("wait for network A healthy");
    ctx.ping_until_healthy(&project_dir_a, "my-network");

    eprintln!("second network run attempt in another project");
    ctx.icp()
        .current_dir(&project_dir_b)
        .args(["network", "run", "my-network"])
        .assert()
        .failure()
        .stderr(contains(format!(
            "Error: port 8080 is in use by the my-network network of the project at '{}'",
            project_dir_a.canonicalize().unwrap().display()
        )));
}

#[test]
#[file_serial(port8001, port8002)]
fn two_projects_different_fixed_ports() {
    let ctx_a = TestContext::new();
    let project_dir_a = ctx_a.create_project_dir("a");

    // Project manifest
    write_string(
        &project_dir_a.join("icp.yaml"), // path
        indoc! {r#"
            networks:
              - name: my-network
                mode: managed
                gateway:
                  port: 8001
        "#}, // contents
    )
    .expect("failed to write project manifest");

    let ctx_b = TestContext::new();
    let project_dir_b = ctx_b.create_project_dir("b");

    // Project manifest
    write_string(
        &project_dir_b.join("icp.yaml"), // path
        indoc! {r#"
            networks:
              - name: my-network
                mode: managed
                gateway:
                  port: 8002
        "#}, // contents
    )
    .expect("failed to write project manifest");

    let _a_guard = ctx_a.start_network_in(&project_dir_a, "my-network");

    eprintln!("wait for network A healthy");
    ctx_a.ping_until_healthy(&project_dir_a, "my-network");

    let _b_guard = ctx_b.start_network_in(&project_dir_b, "my-network");

    eprintln!("wait for network B healthy");
    ctx_b.ping_until_healthy(&project_dir_b, "my-network");
}

// TODO(or.ricon) This is broken
#[test]
fn deploy_to_other_projects_network() {
    let ctx = TestContext::new();

    // Project A
    let proja = ctx.create_project_dir("project-a");

    // Project manifest
    write_string(
        &proja.join("icp.yaml"), // path
        NETWORK_RANDOM_PORT,     // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&proja, "my-network");

    let TestNetwork {
        gateway_port,
        root_key,
        ..
    } = ctx.wait_for_network_descriptor(&proja, "my-network");

    ctx.ping_until_healthy(&proja, "my-network");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project B
    let projb = ctx.create_project_dir("project-b");

    // Connect to Project A's network
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

        networks:
          - name: network-a
            mode: connected
            url: http://localhost:{gateway_port}
            root-key: {root_key}

        environments:
          - name: environment-1
            network: network-a
    "#};

    write_string(
        &projb.join("icp.yaml"), // path
        &pm,                     // contents
    )
    .expect("failed to write project manifest");

    // Deploy project (first time)
    clients::icp(&ctx, &projb, Some("environment-1".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&projb)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--environment",
            "environment-1",
        ])
        .assert()
        .success();

    // Deploy project (second time)
    ctx.icp()
        .current_dir(&projb)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--environment",
            "environment-1",
        ])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&projb)
        .args([
            "canister",
            "call",
            "--environment",
            "environment-1",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[test]
fn network_seeds_preexisting_identities_icp_and_cycles_balances() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(
        &project_dir.join("icp.yaml"), // path
        &formatdoc! {r#"
            {NETWORK_RANDOM_PORT}
            {ENVIRONMENT_RANDOM_PORT}
        "#}, // contents
    )
    .expect("failed to write project manifest");

    let icp_client = clients::icp(&ctx, &project_dir, Some("my-environment".to_string()));

    // Create identities BEFORE starting the network
    icp_client.create_identity("before");

    // Time how long it takes to configure and start the network
    let start = std::time::Instant::now();
    let _guard = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");
    let duration = start.elapsed();
    println!("========== Configuring and starting network took {duration:?}");

    // Create identities AFTER starting the network
    icp_client.create_identity("after");

    // Anonymouys starts with massive initial balance
    icp_client.use_identity("anonymous");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--environment", "my-environment"])
        .assert()
        .stdout(contains("Balance: 1000000000.00000000 ICP"))
        .success();

    // Identities created before starting should have a large seeded ICP balance
    icp_client.use_identity("before");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--environment", "my-environment"])
        .assert()
        .stdout(contains("Balance: 1000000.00000000 ICP"))
        .success();

    // Identities created after starting should have 0 ICP balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--environment", "my-environment"])
        .assert()
        .stdout(contains("Balance: 0 ICP"))
        .success();

    // Identities created before starting should have a large seeded cycles balance
    icp_client.use_identity("before");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "my-environment"])
        .assert()
        .stdout(contains("Balance: 1000.000000000000 TCYCLES"))
        .success();

    // Identities created after starting should have 0 cycles balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "my-environment"])
        .assert()
        .stdout(contains("Balance: 0 TCYCLES"))
        .success();
}

#[tokio::test]
async fn network_run_and_stop_background() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Start network in background and verify we can see child process output
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "run", "my-network", "--background"])
        .assert()
        .success()
        .stdout(contains("Created instance with id")); // part of network start output

    let network = ctx.wait_for_network_descriptor(&project_dir, "my-network");

    // Verify PID file was written
    let pid_file_path = project_dir
        .join(".icp")
        .join("networks")
        .join("my-network")
        .join("background_network_runner.pid");
    assert!(
        pid_file_path.exists(),
        "PID file should exist at {:?}",
        pid_file_path
    );

    let pid_contents = read_to_string(&pid_file_path).expect("Failed to read PID file");
    let background_controller_pid: Pid = pid_contents
        .trim()
        .parse()
        .expect("PID file should contain a valid process ID");

    // Verify network is healthy with agent.status()
    let agent = ic_agent::Agent::builder()
        .with_url(format!("http://127.0.0.1:{}", network.gateway_port))
        .build()
        .expect("Failed to build agent");

    let status = agent.status().await.expect("Failed to get network status");
    assert!(
        matches!(&status.replica_health_status, Some(health) if health == "healthy"),
        "Network should be healthy"
    );

    // Stop the network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "my-network"])
        .assert()
        .success()
        .stdout(contains(format!(
            "Stopping background network (PID: {})",
            background_controller_pid
        )))
        .stdout(contains("Network stopped successfully"));

    // Verify PID file is removed
    assert!(
        !pid_file_path.exists(),
        "PID file should be removed after stopping"
    );

    // Verify controller process is no longer running
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[background_controller_pid]), true);
    assert!(
        system.process(background_controller_pid).is_none(),
        "Process should no longer be running"
    );

    // Verify network is no longer reachable
    let status_result = agent.status().await;
    assert!(
        status_result.is_err(),
        "Network should not be reachable after stopping"
    );
}

#[tokio::test]
async fn network_starts_with_canisters_preset() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(
        &project_dir.join("icp.yaml"), // path
        &formatdoc! {r#"
            {NETWORK_RANDOM_PORT}
            {ENVIRONMENT_RANDOM_PORT}
        "#}, // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _guard = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");

    let pocket_ic = ctx.pocketic();
    let controller = Some(NNS_ROOT_PRINCIPAL);

    // ICP ledger
    let icp_ledger_status = pocket_ic
        .canister_status(ICP_LEDGER_PRINCIPAL, controller)
        .await
        .unwrap();
    assert!(icp_ledger_status.module_hash.is_some());

    // Cycles ledger
    let cycles_ledger_status = pocket_ic
        .canister_status(CYCLES_LEDGER_PRINCIPAL, controller)
        .await
        .unwrap();
    assert!(cycles_ledger_status.module_hash.is_some());

    // Cycles minting
    let cycles_minting_status = pocket_ic
        .canister_status(CYCLES_MINTING_CANISTER_PRINCIPAL, controller)
        .await
        .unwrap();
    assert!(cycles_minting_status.module_hash.is_some());

    // Registry
    let registry_status = pocket_ic
        .canister_status(REGISTRY_PRINCIPAL, controller)
        .await
        .unwrap();
    assert!(registry_status.module_hash.is_some());

    // Internet identity
    let internet_identity_status = pocket_ic
        .canister_status(INTERNET_IDENTITY_PRINCIPAL, controller)
        .await
        .unwrap();
    assert!(internet_identity_status.module_hash.is_some());
}
