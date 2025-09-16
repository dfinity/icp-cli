use crate::common::TestContext;
use icp_fs::fs::write;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};
use serial_test::serial;

mod common;

#[test]
#[serial]
fn canister_update_settings_controllers() {
    let ctx = TestContext::new().with_dfx();

    // Get principals.
    let principal_alice = get_principal(&ctx, "alice");
    let principal_bob = get_principal(&ctx, "bob");

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

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not()),
        );

    // Add controller
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--add-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str())),
        );

    // Add and remove controller.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--add-controller",
            principal_bob.as_str(),
            "--remove-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not())
                .and(contains(principal_bob.as_str())),
        );

    // Remove controller
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--remove-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_bob.as_str()).not()),
        );

    // Add multiple controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--add-controller",
            principal_alice.as_str(),
            "--add-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );

    // Remove multiple controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--remove-controller",
            principal_alice.as_str(),
            "--remove-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not())
                .and(contains(principal_bob.as_str()).not()),
        );

    // Set multiple controllers
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--set-controller",
            principal_alice.as_str(),
            "--set-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query status with identity alice.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister", "--identity", "alice"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("2vxsx-fae").not())
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );
}

fn get_principal(ctx: &TestContext, identity: &str) -> String {
    ctx.icp()
        .args(["identity", "import", identity, "--from-pem"])
        .arg(ctx.make_asset(format!("{identity}.pem").as_str()))
        .assert()
        .success();

    let status = ctx
        .icp()
        .args(["identity", "principal", "--identity", identity])
        .assert()
        .success();

    String::from_utf8(status.get_output().stdout.clone())
        .expect("stdout was not valid UTF-8")
        .trim()
        .to_string()
}

#[test]
#[serial]
fn canister_update_settings_environment_variables() {
    let ctx = TestContext::new().with_dfx();

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

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains("Environment Variables: N/A")),
        );

    // Add multiple environment variables
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--add-environment-variable",
            "var1=value1",
            "--add-environment-variable",
            "var2=value2",
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Environment Variables:"))
                .and(contains("Name: var1, Value: value1"))
                .and(contains("Name: var2, Value: value2")),
        );

    // Add and remove environment variables
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--add-environment-variable",
            "var3=value3",
            "--remove-environment-variable",
            "var1",
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
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
            "update-settings",
            "my-canister",
            "--remove-environment-variable",
            "var2",
            "--remove-environment-variable",
            "var3",
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Environment Variables: N/A")),
        );
}

#[test]
#[serial]
fn canister_update_settings_miscellaneous() {
    let ctx = TestContext::new().with_dfx();

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

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 0")),
        );

    // Update compute allocation
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "update-settings",
            "my-canister",
            "--compute-allocation",
            "1",
            "--memory-allocation",
            "6GiB",
        ])
        .assert()
        .success();

    // Query status
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 1"))
                .and(contains("Memory allocation: 6_442_450_944")),
        );
}
