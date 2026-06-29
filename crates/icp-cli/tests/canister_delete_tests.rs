use indoc::formatdoc;
use predicates::str::contains;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[tokio::test]
async fn canister_delete() {
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
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Verify canister ID is in the id store after deploy
    let id_mapping_path = project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("random-environment.ids.json");
    let id_mapping_before =
        std::fs::read_to_string(&id_mapping_path).expect("ID mapping file should exist");
    assert!(
        id_mapping_before.contains("my-canister"),
        "ID mapping should contain my-canister before deletion"
    );

    // Stop canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Delete canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "delete",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify canister ID is removed from the id store after delete
    let id_mapping_after =
        std::fs::read_to_string(&id_mapping_path).expect("ID mapping file should still exist");
    assert!(
        !id_mapping_after.contains("my-canister"),
        "ID mapping should NOT contain my-canister after deletion"
    );

    // Query status - should fail because canister ID is not found in id store
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("could not find ID for canister"));
}

#[tokio::test]
async fn canister_delete_through_proxy() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--proxy",
            &proxy_cid,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify canister ID exists in id store
    let id_mapping_path = project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("random-environment.ids.json");
    let id_mapping_before =
        std::fs::read_to_string(&id_mapping_path).expect("ID mapping file should exist");
    assert!(
        id_mapping_before.contains("my-canister"),
        "ID mapping should contain my-canister before deletion"
    );

    // Stop canister through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success();

    // Delete canister through proxy
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "delete",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success();

    // Verify canister ID is removed from the id store
    let id_mapping_after =
        std::fs::read_to_string(&id_mapping_path).expect("ID mapping file should still exist");
    assert!(
        !id_mapping_after.contains("my-canister"),
        "ID mapping should NOT contain my-canister after deletion"
    );
}

/// By default, `canister delete` recovers the canister's liquid cycles to the
/// caller's cycles-ledger account before destroying it.
#[tokio::test]
async fn canister_delete_recovers_cycles() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    icp_client.mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Balance right before deletion; recovery should add to this.
    let caller = icp_client.active_principal();
    let balance_before = clients::cycles_ledger(&ctx).balance_of(caller, None).await;

    // Delete without pre-stopping: recovery installs, starts, recovers, then
    // delete auto-stops and destroys the canister.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "delete",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    let balance_after = clients::cycles_ledger(&ctx).balance_of(caller, None).await;
    assert!(
        balance_after > balance_before,
        "expected cycles-ledger balance to increase after recovery: before={balance_before}, after={balance_after}"
    );
}

/// `--no-recover-cycles` deletes immediately without recovering cycles, leaving
/// the caller's cycles-ledger balance unchanged.
#[tokio::test]
async fn canister_delete_no_recover_cycles() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    icp_client.mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    let caller = icp_client.active_principal();
    let balance_before = clients::cycles_ledger(&ctx).balance_of(caller, None).await;

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "delete",
            "my-canister",
            "--environment",
            "random-environment",
            "--no-recover-cycles",
        ])
        .assert()
        .success();

    let balance_after = clients::cycles_ledger(&ctx).balance_of(caller, None).await;
    assert_eq!(
        balance_after, balance_before,
        "balance must be unchanged when recovery is skipped"
    );
}
