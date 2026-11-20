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

/// Every file under `dir`, with its size.
fn files_under(dir: &camino::Utf8Path) -> Vec<(camino::Utf8PathBuf, u64)> {
    let mut out = Vec::new();
    for entry in icp_project::fs::read_dir(dir).expect("failed to list directory") {
        if entry.is_dir() {
            out.extend(files_under(&entry));
        } else {
            let len = std::fs::metadata(&entry)
                .expect("failed to stat file")
                .len();
            out.push((entry, len));
        }
    }
    out
}

/// A pre-built module is the build's artifact where it lies: canisters built
/// from one file share it, and the project's `.icp` holds no copy of it.
#[test]
fn build_adapter_pre_built_path_is_not_copied_into_the_project() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let module = read(&wasm).expect("failed to load wasm test-file");

    // Project manifest: two canisters from the one module
    let pm = formatdoc! {r#"
        canisters:
          - name: first
            build:
              steps:
                - type: pre-built
                  path: '{wasm}'
          - name: second
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
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    // Nothing the build left in the project is anywhere near the module's
    // size, and no scratch directory outlived it
    let left = files_under(&project_dir.join(".icp"));
    assert!(!left.is_empty(), "the build recorded nothing");
    for (file, len) in left {
        assert!(
            len < module.len() as u64 / 2,
            "'{file}' is {len} bytes, and the module is {}",
            module.len()
        );
    }

    // The module is as it was
    assert_eq!(read(&wasm).expect("failed to re-read wasm"), module);
}

/// What a canister was built from is someone else's file, and can change
/// before the canister is installed. The module that was built is then not
/// there to install, and the install says so before it reaches a network.
#[test]
fn build_adapter_pre_built_path_changed_after_build_is_not_installed() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // A module of the project's own
    let module = project_dir.join("module.wasm");
    std::fs::copy(ctx.make_asset("example_icp_mo.wasm"), &module).expect("failed to copy wasm");

    // Project manifest
    let pm = r#"
canisters:
  - name: my-canister
    build:
      steps:
        - type: pre-built
          path: module.wasm
"#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // Change the module, then install without building again
    std::fs::write(&module, b"another module").expect("failed to rewrite wasm");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "install", "my-canister"])
        .assert()
        .failure()
        .stderr(
            contains("module.wasm")
                .and(contains("has changed since"))
                .and(contains("build the canister again")),
        );

    // Remove it, and the install names it as gone
    std::fs::remove_file(&module).expect("failed to remove wasm");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "install", "my-canister"])
        .assert()
        .failure()
        .stderr(
            contains("module.wasm")
                .and(contains("can no longer be read"))
                .and(contains("build the canister again")),
        );
}

/// A script step after a pre-built step finds the module at the output path,
/// and changes the build's own copy of it, never the file the step named.
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
