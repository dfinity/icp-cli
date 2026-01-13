use candid::Principal;
use icp_canister_interfaces::{
    cycles_ledger::CYCLES_LEDGER_PRINCIPAL,
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL, icp_ledger::ICP_LEDGER_PRINCIPAL,
    internet_identity::INTERNET_IDENTITY_PRINCIPAL, registry::REGISTRY_PRINCIPAL,
};
use indoc::{formatdoc, indoc};
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains, is_match},
};
use serde_json::Value;
use serial_test::file_serial;
use sysinfo::{ProcessesToUpdate, System};
use test_tag::tag;

use crate::common::{
    ENVIRONMENT_DOCKER, ENVIRONMENT_RANDOM_PORT, NETWORK_DOCKER, NETWORK_RANDOM_PORT, TestContext,
    TestNetwork, clients,
};
use icp::{
    fs::{read_to_string, write_string},
    prelude::*,
};

mod common;

#[tokio::test]
#[file_serial(default_local_network)]
async fn network_same_port() {
    let ctx = TestContext::new();

    let project_dir_a = ctx.create_project_dir("a");

    // Project manifest
    let pm = indoc! {r#"
        networks:
          - name: sameport-network
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

    let _a_guard = ctx.start_network_in(&project_dir_a, "sameport-network").await;

    eprintln!("wait for network A healthy");
    ctx.ping_until_healthy(&project_dir_a, "sameport-network");

    eprintln!("second network start attempt in another project");
    ctx.icp()
        .current_dir(&project_dir_b)
        .args(["network", "start", "sameport-network"])
        .assert()
        .failure()
        .stderr(contains(format!(
            "Error: port 8080 is in use by the sameport-network network of the project at '{}'",
            project_dir_a.canonicalize().unwrap().display()
        )));
}

#[tokio::test]
#[file_serial(port8001, port8002)]
async fn two_projects_different_fixed_ports() {
    let ctx_a = TestContext::new();
    let project_dir_a = ctx_a.create_project_dir("a");

    // Project manifest
    write_string(
        &project_dir_a.join("icp.yaml"), // path
        indoc! {r#"
            networks:
              - name: fixedport-network
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
              - name: fixedport-network
                mode: managed
                gateway:
                  port: 8002
        "#}, // contents
    )
    .expect("failed to write project manifest");

    let _a_guard = ctx_a.start_network_in(&project_dir_a, "fixedport-network").await;

    eprintln!("wait for network A healthy");
    ctx_a.ping_until_healthy(&project_dir_a, "fixedport-network");

    let _b_guard = ctx_b.start_network_in(&project_dir_b, "fixedport-network").await;

    eprintln!("wait for network B healthy");
    ctx_b.ping_until_healthy(&project_dir_b, "fixedport-network");
}

// TODO(or.ricon) This is broken
#[tokio::test]
async fn deploy_to_other_projects_network() {
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
    let _g = ctx.start_network_in(&proja, "random-network").await;

    let TestNetwork {
        gateway_port,
        root_key,
        ..
    } = ctx.wait_for_network_descriptor(&proja, "random-network");

    ctx.ping_until_healthy(&proja, "random-network");

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
    "#, root_key = hex::encode(&root_key)};

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
            "--subnet",
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
            "--subnet",
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

#[tokio::test]
async fn network_seeds_preexisting_identities_icp_and_cycles_balances() {
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

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));

    // Create identities BEFORE starting the network
    icp_client.create_identity("before");

    // Time how long it takes to configure and start the network
    let start = std::time::Instant::now();
    let _guard = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");
    let duration = start.elapsed();
    println!("========== Configuring and starting network took {duration:?}");

    // Create identities AFTER starting the network
    icp_client.create_identity("after");

    // Anonymouys starts with massive initial balance
    icp_client.use_identity("anonymous");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(is_match(r"Balance: \d{9}\.\d{8} ICP").unwrap())
        .success();

    // Identities created before starting should have a large seeded ICP balance
    icp_client.use_identity("before");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 1000000.00000000 ICP"))
        .success();

    // Identities created after starting should have 0 ICP balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 0 ICP"))
        .success();

    // Identities created before starting should have a large seeded cycles balance
    icp_client.use_identity("before");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 1000.000000000000 TCYCLES"))
        .success();

    // Identities created after starting should have 0 cycles balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
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
        .args([
            "network",
            "start",
            "random-network",
            "--background",
            "--debug",
        ])
        .assert()
        .success()
        .stderr(contains("Seeding ICP and TCYCLES"))
        .stdout(contains("Installed Candid UI canister with ID"));

    let network = ctx.wait_for_network_descriptor(&project_dir, "random-network");

    // Verify network descriptor file was written
    let descriptor_file_path = project_dir
        .join(".icp")
        .join("cache")
        .join("networks")
        .join("random-network")
        .join("descriptor.json");
    assert!(
        descriptor_file_path.exists(),
        "Network descriptor file should exist at {:?}",
        descriptor_file_path
    );

    let descriptor_contents =
        read_to_string(&descriptor_file_path).expect("Failed to read network descriptor file");
    let descriptor: Value = descriptor_contents
        .trim()
        .parse()
        .expect("Descriptor file should contain valid JSON");
    let background_launcher_pid = descriptor
        .get("child-locator")
        .and_then(|cl| cl.get("pid"))
        .and_then(|pid| pid.as_u64())
        .expect("Descriptor should contain launcher PID");
    let background_launcher_pid = (background_launcher_pid as usize).into();

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
        .args(["network", "stop", "random-network"])
        .assert()
        .success()
        .stdout(contains(format!(
            "Stopping background network (PID: {})",
            background_launcher_pid
        )))
        .stdout(contains("Network stopped successfully"));

    // Verify PID file is removed
    assert!(
        !descriptor_file_path.exists(),
        "Descriptor file should be removed after stopping"
    );

    // Verify launcher process is no longer running
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[background_launcher_pid]), true);
    assert!(
        system.process(background_launcher_pid).is_none(),
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
    let _guard = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let agent = ctx.agent();

    // ICP ledger
    agent
        .read_state_canister_module_hash(ICP_LEDGER_PRINCIPAL)
        .await
        .unwrap();

    // Cycles ledger
    agent
        .read_state_canister_module_hash(CYCLES_LEDGER_PRINCIPAL)
        .await
        .unwrap();
    // Cycles minting
    agent
        .read_state_canister_module_hash(CYCLES_MINTING_CANISTER_PRINCIPAL)
        .await
        .unwrap();
    // Registry
    agent
        .read_state_canister_module_hash(REGISTRY_PRINCIPAL)
        .await
        .unwrap();
    // Internet identity
    agent
        .read_state_canister_module_hash(INTERNET_IDENTITY_PRINCIPAL)
        .await
        .unwrap();
}

#[tag(docker)]
#[tokio::test]
async fn network_docker() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("docker-network");

    // Project manifest
    write_string(
        &project_dir.join("icp.yaml"),
        &formatdoc! {r#"
            {NETWORK_DOCKER}
            {ENVIRONMENT_DOCKER}
        "#},
    )
    .expect("failed to write project manifest");

    ctx.docker_pull_network();
    let _guard = ctx.start_network_in(&project_dir, "docker-network").await;
    ctx.ping_until_healthy(&project_dir, "docker-network");

    let balance = clients::ledger(&ctx)
        .balance_of(Principal::anonymous(), None)
        .await;
    assert!(balance > 0_u128);
}

#[tokio::test]
#[file_serial(default_local_network)]
async fn override_local_network_with_custom_port() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("custom-local");

    // Define a custom "local" network with port 9999
    write_string(
        &project_dir.join("icp.yaml"),
        indoc! {r#"
            networks:
              - name: local
                mode: managed
                gateway:
                  port: 9999
        "#},
    )
    .expect("failed to write project manifest");

    // Start the custom "local" network
    let _guard = ctx.start_network_in(&project_dir, "local").await;

    let network = ctx.wait_for_network_descriptor(&project_dir, "local");

    // Verify it's using the custom port
    assert_eq!(network.gateway_port, 9999);

    ctx.ping_until_healthy(&project_dir, "local");
}

#[tokio::test]
async fn cannot_override_mainnet() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("override-mainnet");

    // Attempt to override mainnet
    write_string(
        &project_dir.join("icp.yaml"),
        indoc! {r#"
            networks:
              - name: mainnet
                mode: connected
                url: http://fake-mainnet.local
        "#},
    )
    .expect("failed to write project manifest");

    // Any command that loads the project should fail
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(contains("`mainnet` is a reserved network name"));
}

#[tokio::test]
#[file_serial(default_local_network)]
async fn override_local_network_as_connected() {
    let ctx = TestContext::new();

    // Project A: Start a normal local network with random port
    let project_a = ctx.create_project_dir("project-a");
    write_string(&project_a.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write manifest");

    let _guard = ctx.start_network_in(&project_a, "random-network").await;
    let network_a = ctx.wait_for_network_descriptor(&project_a, "random-network");
    ctx.ping_until_healthy(&project_a, "random-network");

    // Project B: Override "local" to connect to Project A's network
    let project_b = ctx.create_project_dir("project-b");
    write_string(
        &project_b.join("icp.yaml"),
        &formatdoc! {r#"
            networks:
              - name: local
                mode: connected
                url: http://localhost:{port}
                root-key: {root_key}
        "#,
            port = network_a.gateway_port,
            root_key = hex::encode(&network_a.root_key)
        },
    )
    .expect("failed to write manifest");

    // Should be able to use the "local" environment/network name
    // even though it points to a connected network
    ctx.icp()
        .current_dir(&project_b)
        .args(["network", "ping", "local"])
        .assert()
        .success();
}
