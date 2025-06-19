use crate::common::TestEnv;
use icp_fs::fs::write;
use predicates::{ord::eq, str::PredicateStrExt};

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
