use indoc::formatdoc;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[tokio::test]
#[allow(clippy::await_holding_refcell_ref)]
async fn canister_top_up() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
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

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "my-network");
    ctx.ping_until_healthy(&project_dir, "my-network");

    // Create canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create", "--environment", "my-environment"])
        .assert()
        .success();

    let canister_id = clients::icp(&ctx, &project_dir, Some("my-environment".to_string()))
        .get_canister_id("my-canister");

    let canister_balance = ctx.pocketic().cycle_balance(canister_id).await;

    // top up with more cycles than available
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "top-up",
            "my-canister",
            "--environment",
            "my-environment",
            "--amount",
            &format!("{}", 123_456 * TRILLION),
        ])
        .assert()
        .stderr(contains(
            "Failed to top up: Insufficient cycles. Requested: 123456.000000000000T cycles",
        ))
        .failure();

    // top up with reasonable amount
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "top-up",
            "my-canister",
            "--environment",
            "my-environment",
            "--amount",
            &format!("{}", 10 * TRILLION),
        ])
        .assert()
        .stdout(
            eq(format!(
                "Topped up canister my-canister:{canister_id} with 10.000000000000T cycles"
            ))
            .trim(),
        )
        .success();

    let new_canister_balance = ctx.pocketic().cycle_balance(canister_id).await;
    assert_eq!(new_canister_balance, canister_balance + 10 * TRILLION);
}
