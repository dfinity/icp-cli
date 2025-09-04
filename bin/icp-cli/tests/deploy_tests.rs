use crate::common::TestContext;
use icp_fs::fs::write;
use predicates::{ord::eq, str::PredicateStrExt};
use serial_test::serial;

mod common;

#[test]
fn deploy_empty() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
        - canisters/*
    "#;

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            "gnkm6-o3f2j-s4j4o-tn4cp-ebkfd-46tuv-xaitz-fv54k-u7b2d-ejijp-vqe",
        ])
        .assert()
        .success();
}

#[test]
fn deploy_canister_not_found() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
        - canisters/*
    "#;

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--subnet-id",
            "gnkm6-o3f2j-s4j4o-tn4cp-ebkfd-46tuv-xaitz-fv54k-u7b2d-ejijp-vqe",
        ])
        .assert()
        .failure()
        .stderr(eq("Error: project does not contain a canister named 'my-canister'").trim());
}

#[test]
#[serial]
fn deploy() {
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
        .args([
            "deploy",
            "--subnet-id",
            "gnkm6-o3f2j-s4j4o-tn4cp-ebkfd-46tuv-xaitz-fv54k-u7b2d-ejijp-vqe",
        ])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[test]
#[serial]
fn deploy_twice_should_succeed() {
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

    // Deploy project (first time)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            "gnkm6-o3f2j-s4j4o-tn4cp-ebkfd-46tuv-xaitz-fv54k-u7b2d-ejijp-vqe",
        ])
        .assert()
        .success();

    // Deploy project (second time)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet-id",
            "gnkm6-o3f2j-s4j4o-tn4cp-ebkfd-46tuv-xaitz-fv54k-u7b2d-ejijp-vqe",
        ])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
