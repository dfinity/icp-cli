use crate::common::{TestContext, clients};
use icp::{fs::write_string, prelude::*};
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

mod common;

#[tokio::test]
async fn canister_top_up() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = r#"
    canister:
      name: my-canister
      build:
        steps:
          - type: script
            command: echo hi
    "#;

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);

    // Create canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "create"])
        .assert()
        .success();
    let canister_id = clients::icp(&ctx, &project_dir).get_canister_id("my-canister");
    let canister_balance = ctx.pocketic().cycle_balance(canister_id).await;

    // top up with more cycles than available
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "top-up",
            "my-canister",
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
            "--amount",
            &format!("{}", 10 * TRILLION),
        ])
        .assert()
        .stdout(eq("Topped up canister my-canister with 10.000000000000T cycles").trim())
        .success();

    let new_canister_balance = ctx.pocketic().cycle_balance(canister_id).await;
    assert_eq!(new_canister_balance, canister_balance + 10 * TRILLION);
}
