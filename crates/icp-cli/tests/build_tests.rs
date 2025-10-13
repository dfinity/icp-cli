use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::TestContext;
use icp::fs::write_string;

mod common;

#[test]
fn build_adapter_script_single() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {path} "$ICP_WASM_OUTPUT_PATH"'
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn build_adapter_script_multiple() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: echo "before"
              - type: script
                command: sh -c 'cp {path} "$ICP_WASM_OUTPUT_PATH"'
              - type: script
                command: echo "after"
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn build_adapter_display_failing_build_output() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = indoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo "success 1"
                - type: script
                  command: echo "success 2"
                - type: script
                  command: sh -c 'for i in $(seq 1 20); do echo "failing build step $i"; done; exit 1'
          - name: unimportant-canister
            build:
              steps:
                - type: script
                  command: echo "hide this" 
                - type: script
                  command: sh -c 'touch "$ICP_WASM_OUTPUT_PATH"'
        "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains("Build output for canister my-canister"))
        .stdout(contains("Building: script (command: echo \"success 1\") 1 of 3"))
        .stdout(contains("success 1"))
        .stdout(contains("Building: script (command: echo \"success 2\") 2 of 3"))
        .stdout(contains("success 2"))
        .stdout(contains("Building: script (command: sh -c 'for i in $(seq 1 20); do echo \"failing build step $i\"; done; exit 1') 3 of 3"))
        .stdout(contains("failing build step 1"))
        .stdout(contains("failing build step 20"))
        .stdout(contains("hide this").not());
}

#[test]
fn build_adapter_display_failing_prebuilt_output() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with a prebuilt step that will fail (non-existent file)
    let pm = indoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo "initial step succeeded"
                - type: pre-built
                  path: /nonexistent/path/to/wasm.wasm
                  sha256: invalid
        "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains("Build output for canister my-canister"))
        .stdout(contains(
            "Building: script (command: echo \"initial step succeeded\") 1 of 2",
        ))
        .stdout(contains("initial step succeeded"))
        .stdout(contains(
            "Building: pre-built (path: /nonexistent/path/to/wasm.wasm, sha: invalid) 2 of 2",
        ))
        .stdout(contains(
            "Reading local file: /nonexistent/path/to/wasm.wasm",
        ));
}
