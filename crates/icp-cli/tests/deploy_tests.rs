use indoc::{formatdoc, indoc};
use predicates::{
    ord::eq,
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains},
};
use test_tag::tag;

use crate::common::{
    ENVIRONMENT_DOCKER_ENGINE, ENVIRONMENT_RANDOM_PORT, NETWORK_DOCKER_ENGINE, NETWORK_RANDOM_PORT,
    TestContext, build_sync_plugin_example, clients,
};
use icp::{
    fs::{create_dir_all, write_string},
    prelude::*,
    store_id::IdMapping,
};

mod common;

#[test]
fn deploy_empty() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = indoc! {r#"
        canisters:
            - canisters/*
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy"])
        .assert()
        .success();
}

#[test]
fn deploy_canister_not_found() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = indoc! {r#"
        canisters:
            - canisters/*
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "my-canister"])
        .assert()
        .failure()
        .stderr(contains("Error: project does not contain a canister named 'my-canister'").trim());
}

#[tokio::test]
async fn deploy() {
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

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

/// `deploy --no-create` must refuse to create canisters that do not yet exist,
/// failing with a message that names the missing canisters.
#[tokio::test]
async fn deploy_no_create_fails_when_canister_missing() {
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

    // The canister was never created, so --no-create must error instead of creating it.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--environment",
            "random-environment",
            "--no-create",
        ])
        .assert()
        .failure()
        // The error prints without a `Creating canisters:` header in front of it.
        .stderr(
            contains(
                "`--no-create` was specified but the following canisters do not exist: my-canister",
            )
            .and(contains("Creating canisters:").not()),
        );
}

/// `deploy --no-create` succeeds when the canister already exists: it skips
/// creation and proceeds to install as normal.
#[tokio::test]
async fn deploy_no_create_succeeds_when_canister_exists() {
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

    // First deploy creates the canister, so it prints the `Creating canisters:` header.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stderr(contains("Creating canisters:"));

    // With the canister already created, --no-create has nothing to reject and succeeds.
    // Nothing is created, so it reports the canisters exist without a `Creating canisters:` header.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--environment",
            "random-environment",
            "--no-create",
        ])
        .assert()
        .success()
        .stderr(contains("All canisters already exist").and(contains("Creating canisters:").not()));

    // Confirm the canister is installed and callable.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

/// `--no-create` conflicts with the creation-only flags (`--subnet`, `--cycles`)
/// at the clap level, so no network setup is needed. The defaulted `--cycles`
/// only conflicts when passed explicitly, which is what this exercises.
#[tokio::test]
async fn deploy_no_create_conflicts_with_creation_flags() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["deploy", "--no-create", "--cycles", "5t"])
        .assert()
        .failure()
        .stderr(contains("--no-create").and(contains("--cycles")));

    ctx.icp()
        .args(["deploy", "--no-create", "--subnet", "aaaaa-aa"])
        .assert()
        .failure()
        .stderr(contains("--no-create").and(contains("--subnet")));
}

#[tokio::test]
async fn deploy_twice_should_succeed() {
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

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy project (first time)
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Deploy project (second time)
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "greet",
            "(\"test\")",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

/// Verifies that `deploy --subnet <id>` routes the canister to the requested subnet.
///
/// The network is configured with multiple application subnets so the placement is an actual
/// choice: if `--subnet` were ignored (and a subnet picked by default instead), the canister
/// could land on a different one and the assertion would fail.
#[tokio::test]
async fn deploy_routes_canister_to_requested_subnet() {
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

        networks:
          - name: random-network
            mode: managed
            gateway:
              port: 0
            subnets: [application, application]
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let subnet_id = ctx.application_subnet_id().await;

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    icp_client.mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--subnet",
            &subnet_id,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // The canister must end up on exactly the subnet we requested.
    let actual_subnet = clients::registry(&ctx)
        .get_subnet_for_canister(icp_client.get_canister_id("my-canister"))
        .await;
    assert_eq!(
        actual_subnet.to_string(),
        subnet_id,
        "canister should be deployed on the requested subnet"
    );
}

#[tokio::test]
async fn canister_create_colocates_canisters() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-c
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-d
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-e
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-f
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"
        networks:
          - name: random-network
            mode: managed
            gateway:
                port: 0
            subnets: [application, application, application]
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

    // Deploy first three canisters
    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    icp_client.mint_cycles(20 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "canister-a",
            "canister-b",
            "canister-c",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure(); // no valid wasm - should fail but still creates canisters

    let registry = clients::registry(&ctx);

    let subnet_a = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-a"))
        .await;

    let subnet_b = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-b"))
        .await;

    let subnet_c = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-c"))
        .await;

    assert_eq!(
        subnet_a, subnet_b,
        "Canister A and B should be on the same subnet"
    );
    assert_eq!(
        subnet_a, subnet_c,
        "Canister B and C should be on the same subnet"
    );

    // Deploy remaining canisters
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "canister-d",
            "canister-e",
            "canister-f",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure(); // no valid wasm - should fail but still creates canisters

    let subnet_d = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-d"))
        .await;

    let subnet_e = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-e"))
        .await;

    let subnet_f = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-f"))
        .await;

    assert_eq!(
        subnet_a, subnet_d,
        "Canister D should be on the same subnet as canister A"
    );
    assert_eq!(
        subnet_a, subnet_e,
        "Canister E should be on the same subnet as canister A"
    );
    assert_eq!(
        subnet_a, subnet_f,
        "Canister F should be on the same subnet as canister A"
    );
}

#[tokio::test]
async fn deploy_prints_canister_urls() {
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

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy project and verify Candid UI URLs are printed
    // The example_icp_mo.wasm doesn't have http_request, so it should show Candid UI
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stdout(contains("Deployed canisters:"))
        .stdout(contains("my-canister (Candid UI):"))
        .stdout(contains(".localhost:"))
        .stdout(contains("?id="));
}

#[tokio::test]
async fn deploy_prints_friendly_url_for_asset_canister() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with a pre-built asset canister
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  url: https://github.com/dfinity/sdk/raw/refs/tags/0.27.0/src/distributed/assetstorage.wasm.gz
                  sha256: 865eb25df5a6d857147e078bb33c727797957247f7af2635846d65c5397b36a6

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy and check that the friendly URL is printed (not the Candid UI form)
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stdout(contains("Deployed canisters:"))
        .stdout(contains(
            "my-canister: http://my-canister.random-environment.localhost:",
        ));
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_upgrade_rejects_incompatible_candid() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Write mops.toml for the Motoko toolchain
    write_string(
        &project_dir.join("mops.toml"),
        indoc! {r#"
            [dependencies]
            base = "0.16.0"

            [toolchain]
            moc = "0.16.3"
        "#},
    )
    .expect("failed to write mops.toml");

    // Initial version: greet takes Text
    write_string(
        &project_dir.join("main.mo"),
        indoc! {"
            persistent actor {
                public query func greet(name : Text) : async Text {
                    return \"Hello, \" # name # \"!\";
                };
            };
        "},
    )
    .expect("failed to write main.mo");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network and deploy initial version
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify initial version works
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Breaking change: greet now takes Nat instead of Text
    write_string(
        &project_dir.join("main.mo"),
        indoc! {"
            import Nat \"mo:base/Nat\";

            persistent actor {
                public query func greet(n : Nat) : async Text {
                    return \"Hello, \" # Nat.toText(n) # \"!\";
                };
            };
        "},
    )
    .expect("failed to write updated main.mo");

    // Deploy upgrade should fail with candid incompatibility error
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("Candid interface compatibility check failed"));

    // Deploy with --yes should succeed
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--yes",
        ])
        .assert()
        .success();

    // Verify updated version works
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(42)",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, 42!\")").trim());
}

#[tag(docker)]
#[tokio::test]
async fn deploy_cloud_engine() {
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

        {NETWORK_DOCKER_ENGINE}
        {ENVIRONMENT_DOCKER_ENGINE}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.docker_pull_engine_network();
    let _guard = ctx
        .start_network_in(&project_dir, "docker-engine-network")
        .await;
    ctx.ping_until_healthy(&project_dir, "docker-engine-network");

    // Find the CloudEngine subnet by querying the topology endpoint
    // TODO replace with a subnet selection parameter once we have one
    let cloud_engine_subnet_id = ctx.cloud_engine_subnet_id().await;

    // Deploy to the CloudEngine subnet
    // Only the admin can do this. In local envs, the admin is the anonymous principal
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            &cloud_engine_subnet_id,
            "--environment",
            "docker-engine-environment",
        ])
        .assert()
        .success();

    // Query canister to verify it works
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "docker-engine-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_inline_args_candid() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

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
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args",
            "(opt (42 : nat8))",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"42\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_args_overrides_manifest_init_args() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Manifest sets init_args to 7; CLI --args should override it with 42
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            init_args: "(opt (7 : nat8))"
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

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
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args",
            "(opt (42 : nat8))",
        ])
        .assert()
        .success();

    // CLI --args (42) should take priority over manifest init_args (7)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"42\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_args_file() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");
    write_string(&project_dir.join("args.txt"), "(opt (42 : nat8))")
        .expect("failed to write args file");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args-file",
            "args.txt",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"42\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_args_hex_format() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Hex encoding of "(opt 100 : opt nat8)" — didc encode '(opt 100 : opt nat8)'
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args",
            "4449444c016e7b01000164",
            "--args-format",
            "hex",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"100\")").trim());
}

#[test]
fn deploy_with_args_multiple_canisters_fails() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = indoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-b
            build:
              steps:
                - type: script
                  command: echo hi
    "#};

    write_string(&project_dir.join("icp.yaml"), pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "canister-a", "canister-b", "--args", "()"])
        .assert()
        .failure()
        .stderr(contains(
            "--args and --args-file can only be used when deploying a single canister",
        ));
}

#[tokio::test]
async fn deploy_through_proxy() {
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

    // Verify canister works by calling it through proxy (proxy is the controller)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"proxy\")",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, proxy!\")").trim());

    // Verify canister status through proxy shows the proxy as controller
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(contains("Status: Running").and(contains(&proxy_cid)));
}

#[tokio::test]
async fn deploy_with_fixed_controller_principals() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // "aaaaa-aa" is the management canister principal — a convenient fixed value.
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              controllers:
                - "aaaaa-aa"

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

    // The controller list must include both the declared principal and the active identity
    // (2vxsx-fae = anonymous principal). Greenfield injection ensures the caller retains access.
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
                .and(contains("aaaaa-aa"))
                .and(contains("2vxsx-fae")),
        );
}

#[tokio::test]
async fn deploy_with_canister_controller() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Canister "a" lists "b" as a controller by name. Both are deployed together, so all
    // references are resolved by sync_settings_many after creation.
    let pm = formatdoc! {r#"
        canisters:
          - name: a
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              controllers:
                - b
          - name: b
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

    let client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    client.mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    let b_principal = client.get_canister_id("b").to_string();

    // "a"'s controllers must include "b"'s principal and the active identity (2vxsx-fae).
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "a",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("Controllers:")
                .and(contains(b_principal.as_str()))
                .and(contains("2vxsx-fae")),
        );
}

#[tokio::test]
async fn deploy_sync_script_icp_env_vars() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // canister-a verifies env/network vars during deploy; canister-b verifies cross-canister
    // CID visibility during the explicit sync step.
    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "ENV=$ICP_CLI_ENVIRONMENT NET=$ICP_CLI_NETWORK CID=$ICP_CLI_CID B_CID=$ICP_CLI_CID_CANISTER_B"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "B_SEES_A=$ICP_CLI_CID_CANISTER_A"

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
        .args(["--debug", "deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stderr(contains("ENV=random-environment"))
        .stderr(contains("NET=random-network"));

    // Read the assigned canister IDs and verify CID vars and cross-canister visibility.
    let id_mapping: IdMapping = icp::fs::json::load(
        &project_dir
            .join(".icp")
            .join("cache")
            .join("mappings")
            .join("random-environment.ids.json"),
    )
    .expect("failed to read ID mapping");

    let cid_a = id_mapping
        .get("canister-a")
        .expect("canister-a ID not found")
        .to_text();

    let cid_b = id_mapping
        .get("canister-b")
        .expect("canister-b ID not found")
        .to_text();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["--debug", "sync", "--environment", "random-environment"])
        .assert()
        .success()
        .stderr(contains(format!("CID={cid_a}")))
        .stderr(contains(format!("B_CID={cid_b}")))
        .stderr(contains(format!("B_SEES_A={cid_a}")));
}

/// Regression: a canister that enters `icp deploy` non-Running (e.g. parked
/// Stopped by a canister pool, or left Stopped by an earlier interrupted deploy)
/// must be started before the asset sync plugin runs. `install_code` is
/// status-preserving, so without the explicit start the plugin's first canister
/// call would fail with IC0508 ("canister is stopped ... does not have a
/// CallContextManager"). After deploy the canister is left Running.
#[tokio::test]
async fn deploy_starts_stopped_canister_before_sync() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // Seed data the plugin uploads via direct canister calls during sync.
    let seed_data = project_dir.join("seed-data");
    create_dir_all(&seed_data).expect("failed to create seed-data");
    write_string(&seed_data.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // First deploy creates, installs, and syncs; the canister ends Running.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Stop it to simulate a canister handed to deploy in a non-Running state.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Status: Stopped"));

    // Deploy again. install_code leaves the canister Stopped, so deploy must
    // start it before the plugin sync runs. Before the fix this failed with
    // IC0508 inside the sync plugin's first canister call.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // The canister is left Running after a deploy that syncs.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Status: Running"));

    // The plugin's direct canister calls succeeded against the restarted canister.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "show",
            "()",
            "--query",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("apple"));
}
