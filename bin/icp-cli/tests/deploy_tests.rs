use crate::common::TestEnv;
use icp_fs::fs::write;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};
use serial_test::serial;

mod common;

#[test]
fn deploy_empty() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

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
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();
}

#[test]
fn deploy_canister_not_found() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

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
    env.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .failure()
        .stderr(eq("Error: project does not contain a canister named 'my-canister'").trim());
}

#[test]
#[serial]
fn deploy() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Query canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[test]
#[serial]
fn deploy_twice_should_fail() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Deploy project (first time)
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Deploy project (second time)
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .failure()
        .stderr(contains("cannot be installed because the canister is not empty").trim());

    // Query canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
