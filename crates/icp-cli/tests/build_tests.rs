use crate::common::TestContext;
use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use icp::fs::write_string;
use predicates::{prelude::PredicateBooleanExt, str::contains};

mod common;

#[test]
fn build_adapter_script_single() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let f = NamedTempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#,
        f.path()
    );

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

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: echo "before"
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
              - type: script
                command: echo "after"
        "#,
        f.path()
    );

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
    let pm = r#"
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
        "#;

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
        .stdout(contains("Build for canister 'my-canister' failed. Build output:"))
        .stdout(contains("Step 1/3: script (command: echo \"success 1\")"))
        .stdout(contains("success 1"))
        .stdout(contains("Step 2/3: script (command: echo \"success 2\")"))
        .stdout(contains("success 2"))
        .stdout(contains("Step 3/3: script (command: sh -c 'for i in $(seq 1 20); do echo \"failing build step $i\"; done; exit 1')"))
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
    let pm = r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo "initial step succeeded"
                - type: pre-built
                  path: /nonexistent/path/to/wasm.wasm
                  sha256: 0000000000000000000000000000000000000000000000000000000000000000
        "#;

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
        .stdout(contains(
            "Build for canister 'my-canister' failed. Build output:",
        ))
        .stdout(contains(
            "Step 1/2: script (command: echo \"initial step succeeded\")",
        ))
        .stdout(contains("initial step succeeded"))
        .stdout(contains(
            "Step 2/2: pre-built (path: /nonexistent/path/to/wasm.wasm, sha: 0000000000000000000000000000000000000000000000000000000000000000",
        ))
        .stdout(contains("Failed: failed to read file"));
}
