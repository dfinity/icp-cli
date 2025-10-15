use std::io::Read;

use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use indoc::{formatdoc, indoc};
use predicates::str::contains;

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
        Build output:

        Building: step 1 of 3 (script)
        echo "success 1"
          success 1

        Building: step 2 of 3 (script)
        echo "success 2"
          success 2

        Building: step 3 of 3 (script)
        for i in $(seq 1 5); do echo "failing build step $i"; done; exit 1
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
        .stderr(contains("Error: One or more canisters failed to build"));
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
        Build output:

        Building: step 1 of 2 (script)
        echo "initial step succeeded"
          initial step succeeded

        Building: step 2 of 2 (pre-built)
        path: /nonexistent/path/to/wasm.wasm, sha: invalid
          Reading local file: /nonexistent/path/to/wasm.wasm
        Failed to build canister: failed to read prebuilt canister file
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output))
        .stderr(contains("Error: One or more canisters failed to build"));
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
        Build output:

        Building: step 1 of 2 (script)
        echo "step 1 succeeded"
          step 1 succeeded

        Building: step 2 of 2 (script)
        exit 1
          <no output>
        Failed to build canister: command 'exit 1' failed with status code 1
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output))
        .stderr(contains("Error: One or more canisters failed to build"));
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
        Build output:

        Building: step 1 of 2 (script)
        echo "canister-one step 1"
          canister-one step 1

        Building: step 2 of 2 (script)
        echo "canister-one error"; exit 1
          canister-one error
        Failed to build canister: command 'echo "canister-one error"; exit 1' failed with status code 1
    "#};

    let expected_output_two = indoc! {r#"
        Build output:

        Building: step 1 of 2 (script)
        echo "canister-two step 1"
          canister-two step 1

        Building: step 2 of 2 (script)
        echo "canister-two error"; exit 1
          canister-two error
        Failed to build canister: command 'echo "canister-two error"; exit 1' failed with status code 1
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output_one))
        .stdout(contains(expected_output_two))
        .stderr(contains("Error: One or more canisters failed to build"));
}

#[test]
fn build_adapter_script_with_explicit_sh_c() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create temporary file
    let mut f = NamedTempFile::new().expect("failed to create temporary file");
    let path = f.path();

    // Project manifest with explicit sh -c
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                command: sh -c 'echo "nested shell" > {path} && cp {path} "$ICP_WASM_OUTPUT_PATH"'
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
    assert_eq!(contents, "nested shell\n");
}

#[test]
fn build_adapter_display_script_multiple_commands_output() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with multiple commands
    let pm = indoc! {r#"
        canister:
          name: my-canister
          build:
            steps:
              - type: script
                commands:
                  - echo "command 1"
                  - echo "command 2"
                  - echo "command 3"
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    let expected_output = indoc! {r#"
        Build output:

        Building: step 1 of 1 (script)
        echo "command 1"
        echo "command 2"
        echo "command 3"
          command 1
          command 2
          command 3
        Failed to build canister: build did not result in output
    "#};

    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stdout(contains(expected_output));
}
