use crate::common::TestEnv;
use camino_tempfile::NamedUtf8TempFile;
use icp_fs::fs::{create_dir_all, write};
use predicates::{ord::eq, str::PredicateStrExt};

mod common;

#[test]
fn single_canister_project() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        f.path()
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn multi_canister_project() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - my-canister
    "#;

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Canister manifest
    let cm = format!(
        r#"
        name: my-canister
        build:
          adapter:
            type: script
            command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        f.path()
    );

    create_dir_all(project_dir.join("my-canister")).expect("failed to create canister directory");

    write(
        project_dir.join("my-canister/canister.yaml"), // path
        cm,                                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn glob_path() {
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

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Canister manifest
    let cm = format!(
        r#"
        name: my-canister
        build:
          adapter:
            type: script
            command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        f.path()
    );

    create_dir_all(project_dir.join("canisters/my-canister"))
        .expect("failed to create canister directory");

    write(
        project_dir.join("canisters/my-canister/canister.yaml"), // path
        cm,                                                      // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn explicit_path_missing() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - my-canister
    "#;

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(eq("Error: canister path must exist and be a directory \'my-canister\'").trim());
}

#[test]
fn redefine_ic_network_disallowed() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    write(
        project_dir.join("icp.yaml"), // path
        "",                           // contents
    )
    .expect("failed to write project manifest");

    let networks_dir = project_dir.join("networks");
    create_dir_all(&networks_dir).expect("failed to create networks directory");
    // Create a network config for 'ic'
    let network = r#"
    mode: connected
    url: https://icp0.io
    "#;
    std::fs::write(networks_dir.join("ic.yaml"), network).unwrap();

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .failure()
        .stderr(eq("Error: cannot redefine the 'ic' network; the network path 'networks/ic' is invalid").trim());
}

#[test]
fn missing_specific_network() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    write(
        project_dir.join("icp.yaml"), // path
        r#"
        networks:
          - missing
        "#,
    )
    .expect("failed to write project manifest");

    let expected_network_path = project_dir
        .canonicalize()
        .expect("failed to canonicalize project directory")
        .join("missing.yaml");
    let expected_error = format!(
        r#"Error: configuration file for network 'missing' not found at '{}'"#,
        expected_network_path.display()
    );

    env.icp()
        .current_dir(project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .failure()
        .stderr(eq(expected_error).trim());
}
