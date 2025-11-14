use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};

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

// TODO(or.ricon): This test is currently not passing, fix it.
// #[test]
// fn explicit_path_missing() {
//     let ctx = TestContext::new();

//     // Setup project
//     let project_dir = ctx.create_project_dir("icp");

//     // Project manifest
//     let pm = r#"
//     canisters:
//       - my-canister
//     "#;

//     write_string(
//         &project_dir.join("icp.yaml"), // path
//         pm,                            // contents
//     )
//     .expect("failed to write project manifest");

//     // Invoke build
//     ctx.icp()
//         .current_dir(project_dir)
//         .args(["build"])
//         .assert()
//         .failure()
//         .stderr(eq("Error: canister path must exist and be a directory \'my-canister\'").trim());
// }

// TODO(or.ricon): This test is currently not passing, fix it.
// #[test]
// fn redefine_ic_network_disallowed() {
//     let ctx = TestContext::new();

//     // Setup project
//     let project_dir = ctx.create_project_dir("icp");

//     write_string(
//         &project_dir.join("icp.yaml"), // path
//         r#"
//         networks:
//           - name: ic
//             mode: connected
//             url: https://icp0.io
//         "#, // contents
//     )
//     .expect("failed to write project manifest");

//     // Invoke build
//     ctx.icp()
//         .current_dir(project_dir)
//         .args(["deploy", "--subnet", common::SUBNET_ID])
//         .assert()
//         .failure()
//         .stderr(eq("Error: cannot redefine the 'ic' network; the network path 'networks/ic' is invalid").trim());
// }
