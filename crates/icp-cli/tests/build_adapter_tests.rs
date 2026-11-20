use indoc::formatdoc;
use k256::sha2::{Digest, Sha256};
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::{TestContext, spawn_test_server};
use icp_project::fs::{read, write_string};

mod common;

#[test]
fn build_adapter_pre_built_path() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  path: '{wasm}'
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

/// A pre-built step hands its module to the build without copying it, so a
/// script step after it must still find the module at the output path, and
/// must change the build's own copy of it and never the file the step named.
#[test]
fn build_adapter_pre_built_path_then_script_changes_the_output_alone() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let module = read(&wasm).expect("failed to load wasm test-file");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  path: '{wasm}'
                - type: script
                  command: printf appended >> "$ICP_WASM_OUTPUT_PATH"
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // The artifact is the module as the script left it
    let artifact = read(&project_dir.join(".icp/cache/artifacts/my-canister"))
        .expect("failed to read build artifact");
    assert_eq!(artifact, [module.as_slice(), b"appended"].concat());

    // The module the step named is as it was
    assert_eq!(read(&wasm).expect("failed to re-read wasm"), module);
}

#[test]
fn build_adapter_pre_built_path_invalid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let bs = read(&wasm).expect("failed to load wasm test-file");

    // Calculate checksum
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(&bs);
        h.finalize()
    });

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  path: '{wasm}'
                  sha256: invalid
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
        .failure()
        .stderr(
            contains("checksum mismatch")
                .and(contains("expected: invalid"))
                .and(contains(format!("actual: {actual}"))),
        );
}

#[test]
fn build_adapter_pre_built_path_valid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let bs = read(&wasm).expect("failed to load wasm test-file");

    // Calculate checksum
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(&bs);
        h.finalize()
    });

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  path: '{wasm}'
                  sha256: {actual}
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
fn build_adapter_pre_built_url() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let bs = read(&wasm).expect("failed to load wasm test-file");

    // Spawn HTTP server
    let server = spawn_test_server("GET", "/canister.wasm", &bs);
    let addr = server.addr();

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  url: http://{addr}/canister.wasm
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
fn build_adapter_pre_built_url_invalid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let bs = read(&wasm).expect("failed to load wasm test-file");

    // Calculate checksum
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(&bs);
        h.finalize()
    });

    // Spawn HTTP server
    let server = spawn_test_server("GET", "/canister.wasm", &bs);
    let addr = server.addr();

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  url: http://{addr}/canister.wasm
                  sha256: invalid
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
        .failure()
        .stderr(
            contains("checksum mismatch")
                .and(contains("expected: invalid"))
                .and(contains(format!("actual: {actual}"))),
        );
}

#[test]
fn build_adapter_pre_built_url_valid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let bs = read(&wasm).expect("failed to load wasm test-file");

    // Calculate checksum
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(&bs);
        h.finalize()
    });

    // Spawn HTTP server
    let server = spawn_test_server("GET", "/canister.wasm", &bs);
    let addr = server.addr();

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  url: http://{addr}/canister.wasm
                  sha256: {actual}
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
