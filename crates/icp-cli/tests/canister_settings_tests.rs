use crate::common::{
    TestContext,
    clients::{self, IcpCliClient},
};
use icp::{fs::write_string, prelude::*};
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

mod common;

#[test]
fn canister_settings_update_controllers() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Prepare principals.
    let client = clients::icp(&ctx, &project_dir);
    let principal_alice = get_principal(&client, "alice");
    let principal_bob = get_principal(&client, "bob");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    clients::icp(&ctx, &project_dir).mint_cycles(10 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not()),
        );

    // Add controller
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str())),
        );

    // Add and remove controller.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-controller",
            principal_bob.as_str(),
            "--remove-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not())
                .and(contains(principal_bob.as_str())),
        );

    // Remove controller
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--remove-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_bob.as_str()).not()),
        );

    // Add multiple controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-controller",
            principal_alice.as_str(),
            "--add-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );

    // Remove multiple controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--remove-controller",
            principal_alice.as_str(),
            "--remove-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not())
                .and(contains(principal_bob.as_str()).not()),
        );

    // Set multiple controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--set-controller",
            principal_alice.as_str(),
            "--set-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings with identity alice.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--identity",
            "alice",
        ])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("2vxsx-fae").not())
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );
}

fn get_principal(client: &IcpCliClient, identity: &str) -> String {
    client.create_identity(identity);
    client.get_principal(identity).to_string()
}

#[test]
fn canister_settings_update_log_visibility() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Prepare principals.
    let client = clients::icp(&ctx, &project_dir);
    let principal_alice = get_principal(&client, "alice");
    let principal_bob = get_principal(&client, "bob");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    clients::icp(&ctx, &project_dir).mint_cycles(10 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(starts_with("Canister Settings:").and(contains("Log visibility: Controllers")));

    // Set log visibility to controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--log-visibility",
            "public",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(starts_with("Canister Settings:").and(contains("Log visibility: Public")));

    // Add log viewer.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-log-viewer",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Log visibility: Allowed viewers:"))
                .and(contains(principal_alice.as_str())),
        );

    // Add and remove log viewer.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-log-viewer",
            principal_bob.as_str(),
            "--remove-log-viewer",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Log visibility: Allowed viewers:"))
                .and(contains(principal_alice.as_str()).not())
                .and(contains(principal_bob.as_str())),
        );

    // Remove log viewer.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--remove-log-viewer",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Log visibility: Allowed viewers list is empty")),
        );

    // Add multiple log viewers.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-log-viewer",
            principal_alice.as_str(),
            "--add-log-viewer",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Log visibility: Allowed viewers:"))
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );

    // Remove multiple log viewers.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--remove-log-viewer",
            principal_alice.as_str(),
            "--remove-log-viewer",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Log visibility: Allowed viewers list is empty")),
        );

    // Set multiple log viewers.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--set-log-viewer",
            principal_alice.as_str(),
            "--set-log-viewer",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Log visibility: Allowed viewers:"))
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );
}

#[test]
fn canister_settings_update_miscellaneous() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    clients::icp(&ctx, &project_dir).mint_cycles(200 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--cycles",
            &format!("{}", 120 * TRILLION), // 120 TCYCLES because compute allocation is expensive
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Compute allocation: 0"))
                .and(contains("Freezing threshold: 2_592_000"))
                .and(contains("Reserved cycles limit: 5_000_000_000_000"))
                .and(contains("Wasm memory limit: 3_221_225_472"))
                .and(contains("Wasm memory threshold: 0")),
        );

    // Update compute allocation
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--compute-allocation",
            "1",
            "--memory-allocation",
            "6GiB",
            "--freezing-threshold",
            "8640000",
            "--reserved-cycles-limit",
            "6000000000000",
            "--wasm-memory-limit",
            "4GiB",
            "--wasm-memory-threshold",
            "4GiB",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Compute allocation: 1"))
                .and(contains("Memory allocation: 6_442_450_944"))
                .and(contains("Freezing threshold: 8_640_000"))
                .and(contains("Reserved cycles limit: 6_000_000_000_000"))
                .and(contains("Wasm memory limit: 4_294_967_296"))
                .and(contains("Wasm memory threshold: 4_294_967_296")),
        );
}

#[test]
fn canister_settings_update_environment_variables() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    clients::icp(&ctx, &project_dir).mint_cycles(200 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains("Environment Variables:"))
                .and(contains("Name: ICP_CANISTER_ID:my-canister")),
        );

    // Add multiple environment variables
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-environment-variable",
            "var1=value1",
            "--add-environment-variable",
            "var2=value2",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Environment Variables:"))
                .and(contains("Name: var1, Value: value1"))
                .and(contains("Name: var2, Value: value2")),
        );

    // Add and remove environment variables
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--add-environment-variable",
            "var3=value3",
            "--remove-environment-variable",
            "var1",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Environment Variables:"))
                .and(contains("Name: var1, Value: value1").not())
                .and(contains("Name: var2, Value: value2"))
                .and(contains("Name: var3, Value: value3")),
        );

    // Remove multiple environment variables
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--remove-environment-variable",
            "var2",
            "--remove-environment-variable",
            "var3",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "settings", "show", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Settings:")
                .and(contains("Environment Variables:"))
                .and(contains("Name: var2, Value: value2").not())
                .and(contains("Name: var3, Value: value3").not()),
        );
}
