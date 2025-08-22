use crate::common::TestContext;
use candid::{Encode, Nat, Principal};
use icp_fs::fs::write;
use icrc_ledger_types;
use predicates::str::contains;
use serial_test::serial;

mod common;

#[test]
#[serial]
fn token_balance() {
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
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "balance"])
        .assert()
        .stdout(contains("Balance: 0"))
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 0"))
        .success();

    // mint icp to identity
    ctx.icp_ledger()
        .mint_icp(Principal::anonymous(), None, 123456789_u128);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "icp", "balance"])
        .assert()
        .stdout(contains("Balance: 1.23456789"))
        .success();
}
