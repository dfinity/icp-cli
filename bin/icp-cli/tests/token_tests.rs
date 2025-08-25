use crate::common::TestContext;
use candid::Principal;
use icp_fs::fs::write;
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
        .stdout(contains("Balance: 0 ICP"))
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 0 TCYCLES"))
        .success();

    // mint icp to identity
    ctx.icp_ledger()
        .mint_icp(Principal::anonymous(), None, 123456789_u128);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "icp", "balance"])
        .assert()
        .stdout(contains("Balance: 1.23456789 ICP"))
        .success();
}

#[test]
fn token_transfer() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");
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
    let _g = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);

    ctx.icp_().create_identity("alice");
    ctx.icp_().use_identity("alice");
    let alice_principal = ctx.icp_().active_principal();
    ctx.icp_().create_identity("bob");
    ctx.icp_().use_identity("bob");
    let bob_principal = ctx.icp_().active_principal();

    // Initial balance
    ctx.icp_ledger()
        .mint_icp(alice_principal, None, 1_000_000_000_u128); // 10 ICP
    assert_eq!(
        ctx.icp_ledger().balance_of(alice_principal, None),
        1_000_000_000_u128
    );
    assert_eq!(ctx.icp_ledger().balance_of(bob_principal, None), 0_u128);

    // Simple ICP transfer
    ctx.icp_().use_identity("alice");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "transfer", &bob_principal.to_string(), "1.1"])
        .assert()
        .stdout(contains("Transferred 1.1 ICP"))
        .success();
    assert_eq!(
        ctx.icp_ledger().balance_of(alice_principal, None),
        889_990_000_u128
    );
    assert_eq!(
        ctx.icp_ledger().balance_of(bob_principal, None),
        110_000_000_u128
    );

    // Simple cycles transfer
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--icp-amount", "5"])
        .assert()
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "token",
            "cycles",
            "transfer",
            &bob_principal.to_string(),
            "2",
        ])
        .assert()
        .stdout(contains("Transferred 2 TCYCLES"))
        .success();
    ctx.icp_().use_identity("bob");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 2.000000000000 TCYCLES"))
        .success();
}
