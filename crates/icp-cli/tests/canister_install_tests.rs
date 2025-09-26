use crate::common::TestContext;
use icp_fs::fs::write;
use predicates::{ord::eq, str::PredicateStrExt};

mod common;

#[test]
fn canister_install() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

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
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Build canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    // Create canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--quiet", // Set quiet so only the canister ID is output
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();

    // Install canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "install"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
