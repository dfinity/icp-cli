use crate::common::{TestEnv, test_server::spawn_test_server};
use icp_fs::fs::{read, write};
use k256::sha2::{Digest, Sha256};
use predicates::{prelude::PredicateBooleanExt, str::contains};

mod common;

#[test]
fn build_adapter_pre_built_path() {
    let env = TestEnv::new();

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
              type: pre-built
              path: {}
        "#,
        wasm,
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
fn build_adapter_pre_built_url() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");
    let wasm = read(wasm).expect("failed to load wasm test-file");

    // Spawn HTTP server
    let server = spawn_test_server("GET", "/canister.wasm", &wasm);
    let addr = server.addr();

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: pre-built
              url: http://{addr}/canister.wasm
        "#,
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
fn build_adapter_pre_built_url_invalid_checksum() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");
    let wasm = read(wasm).expect("failed to load wasm test-file");

    // Calculate checksum
    let cksm = hex::encode({
        let mut h = Sha256::new();
        h.update(&wasm);
        h.finalize()
    });

    // Spawn HTTP server
    let server = spawn_test_server("GET", "/canister.wasm", &wasm);
    let addr = server.addr();

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: pre-built
              url: http://{addr}/canister.wasm
              sha256: invalid
        "#,
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
        .failure()
        .stderr(
            contains("unexpected checksum")
                .and(contains("expected: invalid"))
                .and(contains(format!("actual: {cksm}"))),
        );
}

#[test]
fn build_adapter_pre_built_url_valid_checksum() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");
    let wasm = read(wasm).expect("failed to load wasm test-file");

    // Calculate checksum
    let cksm = hex::encode({
        let mut h = Sha256::new();
        h.update(&wasm);
        h.finalize()
    });

    // Spawn HTTP server
    let server = spawn_test_server("GET", "/canister.wasm", &wasm);
    let addr = server.addr();

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: pre-built
              url: http://{addr}/canister.wasm
              sha256: {cksm}
        "#,
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
