use std::net::SocketAddr;

use candid::Principal;
use icp_canister_interfaces::{
    cycles_ledger::CYCLES_LEDGER_PRINCIPAL,
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL, icp_ledger::ICP_LEDGER_PRINCIPAL,
    internet_identity::INTERNET_IDENTITY_PRINCIPAL, registry::REGISTRY_PRINCIPAL,
};
use indoc::{formatdoc, indoc};
use predicates::{
    ord::eq,
    prelude::*,
    str::{contains, is_match},
};
use serde_json::Value;
use serial_test::file_serial;
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

    let _a_guard = ctx
        .start_network_in(&project_dir_a, "sameport-network")
        .await;

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
            dunce::canonicalize(&project_dir_a).unwrap().display()
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

    let _a_guard = ctx_a
        .start_network_in(&project_dir_a, "fixedport-network")
        .await;

    eprintln!("wait for network A healthy");
    ctx_a.ping_until_healthy(&project_dir_a, "fixedport-network");

    let _b_guard = ctx_b
        .start_network_in(&project_dir_b, "fixedport-network")
        .await;

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
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

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
        .stdout(contains("Balance: 1_000_000_000_000_000 cycles"))
        .success();

    // Identities created after starting should have 0 cycles balance
    icp_client.use_identity("after");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 0 cycles"))
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
        .stderr(contains("Seeding ICP and cycles"))
        .stdout(contains("Installed Candid UI canister with ID"))
        .stdout(contains("Installed proxy canister with ID"));

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
    #[cfg(unix)]
    let background_launcher_pid = {
        let background_launcher_pid = descriptor
            .get("child-locator")
            .and_then(|cl| cl.get("pid"))
            .and_then(|pid| pid.as_u64())
            .expect("Descriptor should contain launcher PID");
        (background_launcher_pid as usize).into()
    };
    #[cfg(windows)]
    let background_container_id = {
        let background_container_id = descriptor
            .get("child-locator")
            .and_then(|c| c.get("id"))
            .and_then(|cid| cid.as_str())
            .expect("Descriptor should contain launcher container ID");
        background_container_id.to_string()
    };

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
    let mut stop = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "random-network"])
        .assert()
        .success();
    #[cfg(unix)]
    {
        stop = stop.stdout(contains(format!(
            "Stopping background network (PID: {})",
            background_launcher_pid
        )));
    }
    #[cfg(windows)]
    {
        stop = stop.stdout(contains(format!(
            "Stopping background network (container ID: {})",
            &background_container_id[..12]
        )));
    }
    stop.stdout(contains("Network stopped successfully"));

    // Verify descriptor file is removed
    assert!(
        !descriptor_file_path.exists(),
        "Descriptor file should be removed after stopping"
    );

    // Verify launcher process is no longer running
    #[cfg(unix)]
    {
        use sysinfo::{ProcessesToUpdate, System};
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::Some(&[background_launcher_pid]), true);
        assert!(
            system.process(background_launcher_pid).is_none(),
            "Process should no longer be running"
        );
    }
    #[cfg(windows)]
    {
        let output = std::process::Command::new("docker")
            .args(["inspect", &background_container_id])
            .output()
            .expect("Failed to run docker inspect");
        assert!(!output.status.success(), "Container should no longer exist");
    }

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
            networks:
              - name: random-network
                mode: managed
                gateway:
                    port: 0
                ii: true
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
async fn cannot_override_ic() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("override-ic");

    // Attempt to override ic
    write_string(
        &project_dir.join("icp.yaml"),
        indoc! {r#"
            networks:
              - name: ic
                mode: connected
                url: http://fake-ic.local
        "#},
    )
    .expect("failed to write project manifest");

    // Any command that loads the project should fail
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(contains("`ic` is a reserved network name"));
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

/// Test that specifying a launcher version in the manifest causes the network to use that
/// specific version. Verifies the running launcher binary lives in the v12.0.0 cache directory.
#[cfg(unix)]
#[tokio::test]
async fn network_launcher_uses_configured_version() {
    use sysinfo::{ProcessesToUpdate, System};

    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("versioned-launcher");

    write_string(
        &project_dir.join("icp.yaml"),
        indoc! {r#"
            networks:
              - name: versioned-network
                mode: managed
                gateway:
                  port: 0
                version: "v12.0.0"
        "#},
    )
    .expect("failed to write project manifest");

    // Start in background mode. ctx.icp() does NOT set ICP_CLI_NETWORK_LAUNCHER_PATH,
    // so start.rs will resolve the launcher from the version-based cache.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "start", "versioned-network", "--background"])
        .assert()
        .success();

    let _network = ctx.wait_for_network_descriptor(&project_dir, "versioned-network");

    let descriptor_path = project_dir
        .join(".icp")
        .join("cache")
        .join("networks")
        .join("versioned-network")
        .join("descriptor.json");
    let descriptor_contents =
        read_to_string(&descriptor_path).expect("Failed to read network descriptor");
    let descriptor: Value = descriptor_contents
        .trim()
        .parse()
        .expect("Descriptor should contain valid JSON");

    let launcher_pid = descriptor
        .get("child-locator")
        .and_then(|cl| cl.get("pid"))
        .and_then(|pid| pid.as_u64())
        .expect("Descriptor should contain launcher PID");
    let sysinfo_pid = sysinfo::Pid::from(launcher_pid as usize);

    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), true);
    let process = system
        .process(sysinfo_pid)
        .expect("Launcher process should be running");
    let exe_path = process
        .exe()
        .expect("Should be able to read launcher exe path");
    let version_dir_name = exe_path
        .parent()
        .expect("Exe should have a parent directory")
        .file_name()
        .expect("Parent should have a directory name");
    assert_eq!(
        version_dir_name,
        "v12.0.0",
        "Launcher should run from the v12.0.0 version directory, but exe was at: {}",
        exe_path.display()
    );

    // Clean up the background network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "stop", "versioned-network"])
        .assert()
        .success();
}

/// Test that setting autocontainerize=true causes the network launcher to run in Docker
/// even when a native launcher configuration is used.
///
/// This test is skipped on Windows because autocontainerize has no effect there
/// (Docker is always used on Windows).
#[cfg(not(windows))]
#[tag(docker)]
#[tokio::test]
async fn network_autocontainerize_uses_docker() {
    let ctx = TestContext::new();

    // Set autocontainerize to true
    ctx.icp()
        .args(["settings", "autocontainerize", "true"])
        .assert()
        .success();

    let project_dir = ctx.create_project_dir("autocontainerize-test");

    // Use a native launcher configuration (not an explicit docker image)
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Pull the docker image first
    ctx.docker_pull_network();

    // Start the network
    let _guard = ctx.start_network_in(&project_dir, "random-network").await;

    // Verify the descriptor contains a container ID (not a PID)
    let descriptor_file_path = project_dir
        .join(".icp")
        .join("cache")
        .join("networks")
        .join("random-network")
        .join("descriptor.json");

    let descriptor_contents =
        read_to_string(&descriptor_file_path).expect("Failed to read network descriptor file");
    let descriptor: Value = descriptor_contents
        .trim()
        .parse()
        .expect("Descriptor file should contain valid JSON");

    // When running in Docker, the child-locator should have an "id" field (container ID)
    // rather than a "pid" field
    let child_locator = descriptor
        .get("child-locator")
        .expect("Descriptor should have child-locator");

    assert!(
        child_locator.get("id").is_some(),
        "With autocontainerize=true, child-locator should have container 'id', not 'pid'. Got: {child_locator}"
    );
    assert!(
        child_locator.get("pid").is_none(),
        "With autocontainerize=true, child-locator should not have 'pid'. Got: {child_locator}"
    );

    let container_id = child_locator
        .get("id")
        .and_then(|id| id.as_str())
        .expect("Container ID should be a string");

    // Verify the container is running
    let output = std::process::Command::new("docker")
        .args(["inspect", container_id])
        .output()
        .expect("Failed to run docker inspect");
    assert!(
        output.status.success(),
        "Container should be running while network is active"
    );
}

/// Test that a managed network configured with a custom domain accepts requests
/// addressed to that domain. Uses reqwest's `resolve()` to map the domain to
/// 127.0.0.1 without requiring any system DNS configuration.
#[tokio::test]
async fn network_gateway_responds_to_custom_domain() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("custom-domain");

    let domain = "my-app.localhost";

    write_string(
        &project_dir.join("icp.yaml"),
        &formatdoc! {r#"
            networks:
              - name: domain-network
                mode: managed
                gateway:
                  port: 0
                  domains:
                    - {domain}
            environments:
              - name: domain-env
                network: domain-network
        "#},
    )
    .expect("failed to write project manifest");

    let _guard = ctx.start_network_in(&project_dir, "domain-network").await;
    ctx.ping_until_healthy(&project_dir, "domain-network");

    let network = ctx.wait_for_network_descriptor(&project_dir, "domain-network");
    let port = network.gateway_port;

    let client = reqwest::Client::builder()
        .resolve(domain, SocketAddr::from(([127, 0, 0, 1], port)))
        .build()
        .expect("failed to build reqwest client");

    let resp = client
        .get(format!("http://{domain}:{port}/api/v2/status"))
        .send()
        .await
        .expect("request to custom domain failed");

    assert!(
        resp.status().is_success(),
        "gateway should respond successfully on custom domain, got {}",
        resp.status()
    );
}
