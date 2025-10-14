use std::io::Read;

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
                command: cp {path} "$ICP_WASM_OUTPUT_PATH"
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
                  command: for i in $(seq 1 5); do echo "failing build step $i"; done; exit 1
          - name: unimportant-canister
            build:
              steps:
                - type: script
                  command: echo "hide this" 
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"
        "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    let expected_output = indoc! {r#"
        Build output for canister my-canister:

        Building: script (command: echo "success 1") 1 of 3
        success 1

        Building: script (command: echo "success 2") 2 of 3
        success 2

        Building: script (command: for i in $(seq 1 5); do echo "failing build step $i"; done; exit 1) 3 of 3
        failing build step 1
        failing build step 2
        failing build step 3
        failing build step 4
        failing build step 5
        Failed to build canister: command 'for i in $(seq 1 5); do echo "failing build step $i"; done; exit 1' failed with status code 1
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output))
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
    let expected_output = indoc! {r#"
        Build output for canister my-canister:

        Building: script (command: echo "initial step succeeded") 1 of 2
        initial step succeeded

        Building: pre-built (path: /nonexistent/path/to/wasm.wasm, sha: invalid) 2 of 2
        Reading local file: /nonexistent/path/to/wasm.wasm
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output))
        .stdout(contains("Failed to build canister:"));
}

#[test]
fn build_adapter_display_failing_build_output_no_output() {
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
                  command: echo "step 1 succeeded"
                - type: script
                  command: exit 1
        "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    let expected_output = indoc! {r#"
        Build output for canister my-canister:

        Building: script (command: echo "step 1 succeeded") 1 of 2
        step 1 succeeded

        Building: script (command: exit 1) 2 of 2
        <no output>
        Failed to build canister: command 'exit 1' failed with status code 1
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output));
}

#[test]
fn build_adapter_display_multiple_failing_canisters() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with two canisters that both fail
    let pm = indoc! {r#"
        canisters:
          - name: canister-one
            build:
              steps:
                - type: script
                  command: echo "canister-one step 1"
                - type: script
                  command: echo "canister-one error"; exit 1
          - name: canister-two
            build:
              steps:
                - type: script
                  command: echo "canister-two step 1"
                - type: script
                  command: echo "canister-two error"; exit 1
        "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    let expected_output_one = indoc! {r#"
        Build output for canister canister-one:

        Building: script (command: echo "canister-one step 1") 1 of 2
        canister-one step 1

        Building: script (command: echo "canister-one error"; exit 1) 2 of 2
        canister-one error
        Failed to build canister: command 'echo "canister-one error"; exit 1' failed with status code 1
    "#};

    let expected_output_two = indoc! {r#"
        Build output for canister canister-two:

        Building: script (command: echo "canister-two step 1") 1 of 2
        canister-two step 1

        Building: script (command: echo "canister-two error"; exit 1) 2 of 2
        canister-two error
        Failed to build canister: command 'echo "canister-two error"; exit 1' failed with status code 1
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output_one))
        .stdout(contains(expected_output_two));
}

#[test]
fn build_adapter_script_with_pipes_and_redirections() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let mut f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest demonstrating pipes and redirections work without sh -c
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: echo "hello" | cat > {path} && cp {path} "$ICP_WASM_OUTPUT_PATH"
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

    // Verify the file contains the expected output
    let mut contents = String::new();
    f.read_to_string(&mut contents)
        .expect("failed to read temporary file");
    assert_eq!(contents, "hello\n");
}
