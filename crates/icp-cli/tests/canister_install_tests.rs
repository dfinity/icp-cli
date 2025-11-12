use indoc::formatdoc;
use predicates::{ord::eq, str::PredicateStrExt};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[test]
fn canister_install() {
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
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Build canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // Create canister
    clients::icp(&ctx, &project_dir, Some("my-environment".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--quiet", // Set quiet so only the canister ID is output
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    // Install canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "my-canister",
            "--environment",
            "my-environment",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "my-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
