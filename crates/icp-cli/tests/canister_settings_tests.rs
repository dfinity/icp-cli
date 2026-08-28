use indoc::formatdoc;
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::{
    ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext,
    clients::{self, icp_cli},
};
use icp::{
    fs::{create_dir_all, write_string},
    prelude::*,
};

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
        .args(["deploy", "--environment", "random-environment"])
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
        .stdout(contains("controller: 2vxsx-fae").and(contains(principal_alice.as_str()).not()));

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
        .stdout(contains("controller: 2vxsx-fae").and(contains(principal_alice.as_str())));

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
            contains("controller: 2vxsx-fae")
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
        .stdout(contains("controller: 2vxsx-fae").and(contains(principal_bob.as_str()).not()));

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
            contains("controller: 2vxsx-fae")
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
            contains("controller: 2vxsx-fae")
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
            "--remove-all-controllers",
            "--add-controller",
            principal_alice.as_str(),
            "--add-controller",
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
            contains("2vxsx-fae")
                .not()
                .and(contains(principal_alice.as_str()))
                .and(contains(principal_bob.as_str())),
        );
}

fn get_principal(client: &icp_cli::Client<'_>, identity: &str) -> String {
    client.create_identity(identity);
    client.get_principal(identity).to_string()
}

#[tokio::test]
async fn canister_settings_update_through_proxy() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let client = clients::icp(&ctx, &project_dir, None);
    let principal_alice = get_principal(&client, "alice");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--proxy",
            &proxy_cid,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Add controller through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "update",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
            "--add-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Verify the controller was added
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(contains(&proxy_cid).and(contains(principal_alice.as_str())));
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
        .args(["deploy", "--environment", "random-environment"])
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
        .stdout(contains("Log visibility: Controllers"));

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
        .stdout(contains("Log visibility: Public"));

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
            contains("Log visibility: Allowed viewers").and(contains(principal_alice.as_str())),
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
            contains("Log visibility: Allowed viewers")
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
        .stdout(contains(
            "Log visibility: Allowed viewers\n  log viewer list is empty",
        ));

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
            contains("Log visibility: Allowed viewers")
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
        .stdout(contains(
            "Log visibility: Allowed viewers\n  log viewer list is empty",
        ));

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
            contains("Log visibility: Allowed viewers")
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
            "--cycles",
            "120t", // 120T cycles because compute allocation is expensive
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
            contains("Compute allocation: 0")
                .and(contains("Freezing threshold: 2_592_000"))
                .and(contains("Reserved cycles limit: 5_000_000_000_000"))
                .and(contains("Wasm memory limit: 3_221_225_472"))
                .and(contains("Wasm memory threshold: 0")),
        );

    // Update miscellaneous settings.
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
            "100d",
            "--reserved-cycles-limit",
            "6t",
            "--wasm-memory-limit",
            "4GiB",
            "--wasm-memory-threshold",
            "4GiB",
            "--log-memory-limit",
            "1MiB",
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
            contains("Compute allocation: 1")
                .and(contains("Memory allocation: 6_442_450_944"))
                .and(contains("Freezing threshold: 8_640_000"))
                .and(contains("Reserved cycles limit: 6_000_000_000_000"))
                .and(contains("Wasm memory limit: 4_294_967_296"))
                .and(contains("Wasm memory threshold: 4_294_967_296"))
                .and(contains("Log memory limit: 1_048_576")),
        );
}

/// An environment variable whose value is declared as `{ path: <file> }` is read
/// from that file — relative to the canister's directory, with surrounding
/// whitespace trimmed — and synced to the canister on deploy.
#[tokio::test]
async fn canister_settings_environment_variable_from_file() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            settings:
              environment_variables:
                API_KEY:
                  path: ./secrets/api-key
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");
    create_dir_all(&project_dir.join("secrets")).expect("failed to create the secrets directory");
    write_string(&project_dir.join("secrets/api-key"), "s3cret\n")
        .expect("failed to write the api key file");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(200 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

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
        .stdout(contains("API_KEY: s3cret"));
}

/// A file the manifest points at but that does not exist is reported before
/// anything is deployed, naming the variable and the canister.
#[tokio::test]
async fn canister_settings_environment_variable_file_missing() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            settings:
              environment_variables:
                API_KEY:
                  path: ./secrets/api-key
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(contains("environment variable 'API_KEY'").and(contains("my-canister")));
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
        .args(["deploy", "--environment", "random-environment"])
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
            contains("controller: 2vxsx-fae")
                .and(contains("Environment variables:"))
                .and(contains("PUBLIC_CANISTER_ID:my-canister")),
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
            contains("Environment variables:")
                .and(contains("var1: value1"))
                .and(contains("var2: value2")),
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
            contains("Environment variables:")
                .and(contains("var1: value1").not())
                .and(contains("var2: value2"))
                .and(contains("var3: value3")),
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
            contains("Environment variables:")
                .and(contains("var2: value2").not())
                .and(contains("var3: value3").not()),
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
        .args(["deploy", "--environment", "random-environment"])
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
        .stderr(contains("Wasm memory limit is already set in icp.yaml"));
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

#[tokio::test]
async fn canister_settings_sync_log_visibility() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest without log_visibility
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
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Helper to check log visibility
    fn confirm_log_visibility(ctx: &TestContext, project_dir: &Path, expected: &str) {
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
            .stdout(contains(format!("Log visibility: {}", expected)));
    }

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

    // Default log visibility should be Controllers
    confirm_log_visibility(&ctx, &project_dir, "Controllers");

    // Project manifest with log_visibility: public
    let pm_with_public_log_visibility = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              log_visibility: public

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    // Setting log_visibility in manifest and syncing should update canister settings
    write_string(
        &project_dir.join("icp.yaml"),
        &pm_with_public_log_visibility,
    )
    .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_log_visibility(&ctx, &project_dir, "Public");

    // Project manifest with log_visibility: controllers
    let pm_with_controllers_log_visibility = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              log_visibility: controllers

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    // Changing log_visibility back to controllers should work
    write_string(
        &project_dir.join("icp.yaml"),
        &pm_with_controllers_log_visibility,
    )
    .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_log_visibility(&ctx, &project_dir, "Controllers");

    // Project manifest with log_visibility: allowed_viewers
    let pm_with_allowed_viewers = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              log_visibility:
                allowed_viewers:
                  - "aaaaa-aa"
                  - "2vxsx-fae"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    // Setting allowed_viewers should work
    write_string(&project_dir.join("icp.yaml"), &pm_with_allowed_viewers)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_log_visibility(
        &ctx,
        &project_dir,
        "Allowed viewers\n  log viewer: 2vxsx-fae\n  log viewer: aaaaa-aa",
    );

    // status_visibility takes the same manifest forms, and a single sync has to
    // apply both settings: either change alone would satisfy the "settings
    // already match" check that decides whether to send an update at all.
    let pm_with_both = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              log_visibility: public
              status_visibility:
                allowed_viewers:
                  - "aaaaa-aa"
                  - "2vxsx-fae"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm_with_both)
        .expect("failed to write project manifest");
    sync(&ctx, &project_dir);
    confirm_log_visibility(&ctx, &project_dir, "Public");
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
        .stdout(contains(
            "Status visibility: Allowed viewers\n  status viewer: 2vxsx-fae\n  status viewer: aaaaa-aa",
        ));
}

/// Drives the `--*-status-viewer` / `--status-visibility` flags against a live
/// replica, checking each one both in `settings show` and in what it actually
/// grants: whether a non-controller may read the status, or falls back to the
/// public state-tree information.
///
/// The flag-resolution matrix itself is unit-tested in
/// `commands::canister::settings::update`; what this adds is that clap wires the
/// flags to the right group and that the replica honours the result.
#[tokio::test]
async fn canister_settings_update_status_visibility() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let client = clients::icp(&ctx, &project_dir, None);
    let principal_alice = get_principal(&client, "alice");
    let principal_bob = get_principal(&client, "bob");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    fn update(ctx: &TestContext, project_dir: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
        let mut all = vec![
            "canister",
            "settings",
            "update",
            "my-canister",
            "--environment",
            "random-environment",
        ];
        all.extend_from_slice(args);
        ctx.icp()
            .current_dir(project_dir)
            .args(all)
            .assert()
            .success()
    }

    fn confirm(ctx: &TestContext, project_dir: &Path) -> assert_cmd::assert::Assert {
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
    }

    /// `canister status` as alice, who is not a controller: the full report only
    /// when she may read the status, the public fallback otherwise.
    fn status_as_alice(ctx: &TestContext, project_dir: &Path) -> assert_cmd::assert::Assert {
        ctx.icp()
            .current_dir(project_dir)
            .args([
                "canister",
                "status",
                "my-canister",
                "--identity",
                "alice",
                "--environment",
                "random-environment",
            ])
            .assert()
            .success()
    }

    // The default is controllers, reported separately from log visibility.
    confirm(&ctx, &project_dir).stdout(
        contains("Status visibility: Controllers").and(contains("Log visibility: Controllers")),
    );
    status_as_alice(&ctx, &project_dir).stdout(contains("Status:").not());

    // --add-status-viewer grants it to alice, relative to the current list.
    update(
        &ctx,
        &project_dir,
        &["--add-status-viewer", principal_alice.as_str()],
    );
    status_as_alice(&ctx, &project_dir)
        .stdout(contains("Status: Running").and(contains("Status visibility: Allowed viewers")));

    // Add and remove in one call, again relative to the current list. Alice
    // loses access, so the fallback comes back.
    update(
        &ctx,
        &project_dir,
        &[
            "--add-status-viewer",
            principal_bob.as_str(),
            "--remove-status-viewer",
            principal_alice.as_str(),
        ],
    );
    confirm(&ctx, &project_dir).stdout(
        contains("Status visibility: Allowed viewers")
            .and(contains(principal_bob.as_str()))
            .and(contains(principal_alice.as_str()).not()),
    );
    status_as_alice(&ctx, &project_dir).stdout(contains("Status:").not());

    // --set-status-viewer replaces the list outright.
    update(
        &ctx,
        &project_dir,
        &["--set-status-viewer", principal_alice.as_str()],
    );
    confirm(&ctx, &project_dir).stdout(
        contains("Status visibility: Allowed viewers")
            .and(contains(principal_alice.as_str()))
            .and(contains(principal_bob.as_str()).not()),
    );

    // Public grants it to everyone, and leaves log visibility alone.
    update(&ctx, &project_dir, &["--status-visibility", "public"]);
    confirm(&ctx, &project_dir)
        .stdout(contains("Status visibility: Public").and(contains("Log visibility: Controllers")));
    status_as_alice(&ctx, &project_dir).stdout(contains("Status: Running"));

    // Naming a viewer while the status is public takes access away from
    // everyone else, which is warned about but not prompted for.
    update(
        &ctx,
        &project_dir,
        &["--add-status-viewer", principal_bob.as_str()],
    )
    .stderr(contains(
        "Status visibility is currently public; listing allowed viewers revokes access for everyone else",
    ));
    status_as_alice(&ctx, &project_dir).stdout(contains("Status:").not());

    // So does removing the last viewer, which leaves the controllers alone with it.
    update(
        &ctx,
        &project_dir,
        &["--remove-status-viewer", principal_bob.as_str()],
    )
    .stderr(contains(
        "Status visibility is left with no allowed viewers; only the controllers keep access",
    ));

    // Revoking it puts the fallback back in place.
    update(&ctx, &project_dir, &["--status-visibility", "controllers"]);
    status_as_alice(&ctx, &project_dir).stdout(contains("Status:").not());

    // An update naming neither visibility group must leave both alone, rather
    // than resetting them to an empty allowed-viewers list.
    update(
        &ctx,
        &project_dir,
        &["--set-log-viewer", principal_alice.as_str()],
    );
    update(&ctx, &project_dir, &["--freezing-threshold", "7d"]);
    confirm(&ctx, &project_dir).stdout(
        contains("Status visibility: Controllers")
            .and(contains("Log visibility: Allowed viewers"))
            .and(contains(principal_alice.as_str())),
    );
}

#[tokio::test]
async fn canister_settings_sync_through_proxy() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--proxy",
            &proxy_cid,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Update manifest with memory_allocation setting
    let pm_with_settings = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              memory_allocation: 10485760

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm_with_settings)
        .expect("failed to write project manifest");

    // Sync settings through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success();

    // Verify the setting was applied
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(contains("Memory allocation: 10_485_760"));
}

#[tokio::test]
async fn canister_settings_show() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Output starts with settings fields, not canister identity headers,
    // and does not include fields from the full canister_status result.
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
            contains("Controllers:")
                .and(contains("Compute allocation:"))
                .and(contains("Memory allocation:"))
                .and(contains("Freezing threshold:"))
                .and(contains("Reserved cycles limit:"))
                .and(contains("Wasm memory limit:"))
                .and(contains("Wasm memory threshold:"))
                .and(contains("Log memory limit:"))
                .and(contains("Log visibility:"))
                .and(contains("Environment variables:")),
        );

    // JSON output contains settings field names.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
            "--json",
        ])
        .assert()
        .success()
        .stdout(
            contains(r#""controllers""#)
                .and(contains(r#""compute_allocation""#))
                .and(contains(r#""memory_allocation""#))
                .and(contains(r#""freezing_threshold""#))
                .and(contains(r#""reserved_cycles_limit""#))
                .and(contains(r#""wasm_memory_limit""#))
                .and(contains(r#""wasm_memory_threshold""#))
                .and(contains(r#""log_memory_limit""#))
                .and(contains(r#""log_visibility""#))
                .and(contains(r#""status_visibility""#))
                .and(contains(r#""environment_variables""#)),
        );
}

#[tokio::test]
async fn canister_settings_show_not_a_controller() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let client = clients::icp(&ctx, &project_dir, None);
    let principal_alice = get_principal(&client, "alice");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

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

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Transfer sole ownership to alice, removing the default identity as controller.
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
            "--remove-all-controllers",
            "--add-controller",
            principal_alice.as_str(),
        ])
        .assert()
        .success();

    // Default identity is no longer a controller — command must fail, no fallback.
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
        .failure();

    // Alice is a controller — command succeeds.
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
        .stdout(contains(principal_alice.as_str()));
}
