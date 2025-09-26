use crate::common::TestContext;
use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use icp_fs::fs::write;

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

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
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

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}
