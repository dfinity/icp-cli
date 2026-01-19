use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use predicates::str::contains;

use crate::common::TestContext;
use icp::fs::{create_dir_all, write_string};

mod common;

#[test]
fn single_canister_project() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp {path} "$ICP_WASM_OUTPUT_PATH"
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();
}

#[test]
fn multi_canister_project() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - my-canister
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Canister manifest
    let cm = formatdoc! {r#"
        name: my-canister
        build:
          steps:
            - type: script
              command: cp {path} "$ICP_WASM_OUTPUT_PATH"
    "#};

    create_dir_all(&project_dir.join("my-canister")).expect("failed to create canister directory");

    write_string(
        &project_dir.join("my-canister/canister.yaml"), // path
        &cm,                                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();
}

#[test]
fn glob_path() {
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

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Canister manifest
    let cm = formatdoc! {r#"
        name: my-canister
        build:
          steps:
            - type: script
              command: cp {path} "$ICP_WASM_OUTPUT_PATH"
    "#};

    create_dir_all(&project_dir.join("canisters/my-canister"))
        .expect("failed to create canister directory");

    write_string(
        &project_dir.join("canisters/my-canister/canister.yaml"), // path
        &cm,                                                      // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();
}

#[test]
fn explicit_path_missing() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - my-canister
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke project show
    ctx.icp()
        .current_dir(project_dir)
        .args(["project", "show"])
        .assert()
        .failure()
        .stderr(contains(
            "could not locate a canister manifest at: 'my-canister'",
        ));
}

#[test]
fn explicit_path_missing_canister_yaml() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - my-canister
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Create directory but no canister.yaml
    create_dir_all(&project_dir.join("my-canister")).expect("failed to create canister directory");

    // Invoke project show
    ctx.icp()
        .current_dir(project_dir)
        .args(["project", "show"])
        .assert()
        .failure()
        .stderr(contains(
            "could not locate a canister manifest at: 'my-canister'",
        ));
}

#[test]
fn explicit_path_with_subdirectory() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - canisters/backend
      - canisters/frontend
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Backend canister manifest
    let backend_cm = indoc! {r#"
        name: backend
        build:
          steps:
            - type: script
              command: echo "build"
    "#};

    create_dir_all(&project_dir.join("canisters/backend"))
        .expect("failed to create backend directory");

    write_string(
        &project_dir.join("canisters/backend/canister.yaml"),
        backend_cm,
    )
    .expect("failed to write backend manifest");

    // Frontend canister manifest
    let frontend_cm = indoc! {r#"
        name: frontend
        build:
          steps:
            - type: script
              command: echo "build"
    "#};

    create_dir_all(&project_dir.join("canisters/frontend"))
        .expect("failed to create frontend directory");

    write_string(
        &project_dir.join("canisters/frontend/canister.yaml"),
        frontend_cm,
    )
    .expect("failed to write frontend manifest");

    // Invoke project show - should succeed
    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "show"])
        .assert()
        .success();
}

#[test]
fn redefine_ic_network_disallowed() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("icp.yaml"),
        r#"
        networks:
          - name: ic
            mode: connected
            url: https://fake-ic.io
        "#,
    )
    .expect("failed to write project manifest");

    // Any command that loads the project should fail
    ctx.icp()
        .current_dir(project_dir)
        .args(["project", "show"])
        .assert()
        .failure()
        .stderr(contains("`ic` is a reserved network name"));
}
