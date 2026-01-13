use indoc::{formatdoc, indoc};
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

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
        .args(["deploy", "--subnet", common::SUBNET_ID])
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
        .args(["deploy", "my-canister", "--subnet", common::SUBNET_ID])
        .assert()
        .failure()
        .stderr(eq("Error: project does not contain a canister named 'my-canister'").trim());
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
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

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
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
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
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

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
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Deploy project (second time)
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

#[tokio::test]
async fn canister_create_colocates_canisters() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = indoc! {r#"
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

    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx
        .start_network_with_flags(
            &project_dir,
            &[
                "--subnet=application",
                "--subnet=application",
                "--subnet=application",
            ],
        )
        .await;

    ctx.ping_until_healthy(&project_dir, "local");

    // Deploy first three canisters
    let icp_client = clients::icp(&ctx, &project_dir, None);
    icp_client.mint_cycles(20 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "canister-a", "canister-b", "canister-c"])
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
        .args(["deploy", "canister-d", "canister-e", "canister-f"])
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
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

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
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Deployed canisters:"))
        .stdout(contains("my-canister (Candid UI):"))
        .stdout(contains(".localhost:"))
        .stdout(contains("?id="));
}
