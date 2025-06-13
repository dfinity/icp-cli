use crate::common::TestEnv;
use icp_fs::fs::{create_dir_all, write};
use predicates::{ord::eq, str::PredicateStrExt};

mod common;

#[test]
fn build_adapter_script_simple() {
    let env = TestEnv::new();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canisters:
      - my-canister
    "#;

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Canister manifest
    let cm = r#"
    name: my-canister
    build:
      adapter:
        type: script
        command: echo hi
    "#;

    create_dir_all(project_dir.join("my-canister")).expect("failed to create canister directory");

    write(
        project_dir.join("my-canister/canister.yaml"), // path
        cm,                                            // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    env.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success()
        .stdout(eq("hi").trim());
}
