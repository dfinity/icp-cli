use indoc::formatdoc;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{contains, starts_with},
};

use crate::common::{
    ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext,
    clients::{self, icp_cli},
};
use icp::{fs::write_string, prelude::*};

mod common;

#[tokio::test]
async fn canister_settings_update_controllers() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Prepare principals.
    let client = clients::icp(&ctx, &project_dir, None);
    let principal_alice = get_principal(&client, "alice");
    let principal_bob = get_principal(&client, "bob");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
            "--add-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
            "--remove-controller",
            principal_bob.as_str(),
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains(principal_alice.as_str()).not())
                .and(contains(principal_bob.as_str()).not()),
        );

    // Set multiple controllers (uses --force since we're removing ourselves as controller)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--environment",
            "random-environment",
            "--force",
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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("2vxsx-fae").not())
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );
}

fn get_principal(client: &icp_cli::Client<'_>, identity: &str) -> String {
    client.create_identity(identity);
    client.get_principal(identity).to_string()
}

#[tokio::test]
async fn canister_settings_update_log_visibility() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Prepare principals.
    let client = clients::icp(&ctx, &project_dir, None);
    let principal_alice = get_principal(&client, "alice");
    let principal_bob = get_principal(&client, "bob");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(starts_with("Canister Id:").and(contains("Log visibility: Controllers")));

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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(starts_with("Canister Id:").and(contains("Log visibility: Public")));

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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Log visibility: Allowed viewers:"))
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );
}

#[tokio::test]
async fn canister_settings_update_miscellaneous() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(200 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--cycles",
            &format!("{}", 120 * TRILLION), // 120 TCYCLES because compute allocation is expensive
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Compute allocation: 1"))
                .and(contains("Memory allocation: 6_442_450_944"))
                .and(contains("Freezing threshold: 8_640_000"))
                .and(contains("Reserved cycles limit: 6_000_000_000_000"))
                .and(contains("Wasm memory limit: 4_294_967_296"))
                .and(contains("Wasm memory threshold: 4_294_967_296")),
        );
}

#[tokio::test]
async fn canister_settings_update_environment_variables() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(200 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query settings
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Controllers: 2vxsx-fae"))
                .and(contains("Environment Variables:"))
                .and(contains("Name: PUBLIC_CANISTER_ID:my-canister")),
        );

    // Add multiple environment variables
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
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
            "--environment",
            "random-environment",
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
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            starts_with("Canister Id:")
                .and(contains("Environment Variables:"))
                .and(contains("Name: var2, Value: value2").not())
                .and(contains("Name: var3, Value: value3").not()),
        );
}

#[tokio::test]
async fn canister_settings_sync() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(200 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Test helpers for syncing settings and checking wasm memory limit
    fn sync(ctx: &TestContext, project_dir: &Path) {
        ctx.icp()
            .current_dir(project_dir)
            .args([
                "canister",
                "settings",
                "sync",
                "my-canister",
                "--environment",
                "random-environment",
            ])
            .assert()
            .success();
    }

    fn confirm_wasm_memory_limit(ctx: &TestContext, project_dir: &Path, expected_limit: &str) {
        ctx.icp()
            .current_dir(project_dir)
            .args([
                "canister",
                "settings",
                "show",
                "my-canister",
                "--environment",
                "random-environment",
            ])
            .assert()
            .success()
            .stdout(contains(format!("Wasm memory limit: {}", expected_limit)));
    }

    // Initial value
    confirm_wasm_memory_limit(&ctx, &project_dir, "3_221_225_472");

    let pm_with_empty_settings = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    let pm_with_empty_wasm_limit = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              wasm_memory_limit: ~

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    let pm_with_wasm_limit_4gb = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              wasm_memory_limit: 4000000000

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    // Syncing a nonexistent setting should not override the default
    write_string(&project_dir.join("icp.yaml"), &pm_with_empty_settings)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "3_221_225_472");
    write_string(&project_dir.join("icp.yaml"), &pm_with_empty_wasm_limit)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "3_221_225_472");
    // Setting wasm memory limit in the manifest and syncing should update the canister settings
    write_string(&project_dir.join("icp.yaml"), &pm_with_wasm_limit_4gb)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "4_000_000_000");
    // Existing settings should be overridden on sync
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--environment",
            "random-environment",
            "--wasm-memory-limit",
            "5GiB",
        ])
        .assert()
        .success()
        .stdout(contains("Wasm memory limit is already set in icp.yaml"));
    confirm_wasm_memory_limit(&ctx, &project_dir, "5_368_709_120");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "4_000_000_000");
    // Syncing a nonexistent setting should not override a previously set setting
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "4_000_000_000");
    write_string(&project_dir.join("icp.yaml"), &pm_with_empty_settings)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "4_000_000_000");
    write_string(&project_dir.join("icp.yaml"), &pm_with_empty_wasm_limit)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "4_000_000_000");
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--environment",
            "random-environment",
            "--wasm-memory-limit",
            "5GiB",
        ])
        .assert()
        .success();
    sync(&ctx, &project_dir);
    confirm_wasm_memory_limit(&ctx, &project_dir, "5_368_709_120");
}
