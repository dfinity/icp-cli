use indoc::{indoc, formatdoc};
use predicates::{ord::eq, str::PredicateStrExt};

use icp::{fs::write_string, prelude::*};
use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};

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
        .args(["deploy", "--subnet-id", common::SUBNET_ID])
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
        .args(["deploy", "my-canister", "--subnet-id", common::SUBNET_ID])
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
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {wasm} "$ICP_WASM_OUTPUT_PATH"'

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");

    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
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
            "my-environment",
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
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {wasm} "$ICP_WASM_OUTPUT_PATH"'

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");

    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    // Deploy project (first time)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    // Deploy project (second time)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            common::SUBNET_ID,
            "--environment",
            "my-environment",
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
            "my-environment",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
