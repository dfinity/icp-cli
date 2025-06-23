use crate::common::TestEnv;
use icp_fs::fs::write;
use predicates::{ord::eq, str::PredicateStrExt};
use serial_test::serial;

mod common;

#[test]
#[serial]
fn canister_create() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canister:
      name: my-canister
      build:
        adapter:
          type: script
          command: echo hi
    "#;

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

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();
}

#[test]
#[serial]
fn canister_install() {
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

    // Build canister
    env.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    // Create canister
    let out = env
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--quiet", // Set quiet so only the canister ID is output
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();

    let cid =
        String::from_utf8(out.get_output().stdout.to_owned()).expect("failed to read canister id");

    // Trim newline
    let cid = cid.trim();

    // Install canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "install"])
        .assert()
        .success();

    // Query canister
    env.dfx()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--network",
            "http://localhost:8000",
            cid,
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
