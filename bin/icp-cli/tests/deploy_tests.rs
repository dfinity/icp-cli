use crate::common::TestEnv;
use icp_fs::fs::write;
use serial_test::serial;

mod common;

#[test]
#[serial]
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
        .args(["deploy"])
        .assert()
        .success();
}
