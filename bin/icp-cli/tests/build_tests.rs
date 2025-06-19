use crate::common::TestEnv;
use camino_tempfile::NamedUtf8TempFile;
use icp_fs::fs::write;

mod common;

#[test]
fn build_adapter_script() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        f.path()
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
