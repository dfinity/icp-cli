use indoc::formatdoc;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

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
    let _g = ctx.start_network_in(&project_dir, "random-network");
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Build canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--quiet", // Set quiet so only the canister ID is output
            "--environment",
            "random-environment",
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
            "random-environment",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[test]
fn canister_install_with_valid_principal() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Valid principal
    let principal = "aaaaa-aa";

    // Try to install with principal (should fail without --wasm flag)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            principal,
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains(
            "Cannot install canister by principal without --wasm flag",
        ));
}

#[test]
fn canister_install_with_wasm_flag() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_path = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with a different build command that won't produce a valid wasm
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network");
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Install canister using --wasm flag
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "my-canister",
            "--wasm",
            wasm_path.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify the installation by calling the canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}
