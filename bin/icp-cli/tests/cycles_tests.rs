use crate::common::{TestContext, clients};
use icp_fs::fs::write;
use predicates::str::contains;

mod common;

#[tokio::test]
async fn cycles_balance() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write(
        project_dir.join("icp.yaml"), // path
        "",                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);

    // Wait for network
    ctx.ping_until_healthy(&project_dir);

    // Empty account has empty balance
    let identity = clients::icp_client(&ctx).use_new_random_identity();
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 0 TCYCLES"))
        .success();

    // Mint ICP to cycles, specify ICP amount
    clients::icp_ledger(&ctx)
        .mint_icp(identity, None, 123456789_u64)
        .await;
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--icp", "1"])
        .assert()
        .stdout(contains(
            "Minted 3.519900000000 TCYCLES to your account, new balance: 3.519900000000 TCYCLES.",
        ))
        .success();

    // Mint ICP to cycles, specify cycles amount
    let identity = clients::icp_client(&ctx).use_new_random_identity();
    clients::icp_ledger(&ctx)
        .mint_icp(identity, None, 123456789_u64)
        .await;
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--cycles", "1000000000"])
        .assert()
        .stdout(contains(
            "Minted 0.001000000000 TCYCLES to your account, new balance: 0.001000000000 TCYCLES.",
        ))
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--cycles", "1500000000"])
        .assert()
        .stdout(contains(
            "Minted 0.001500016000 TCYCLES to your account, new balance: 0.002500016000 TCYCLES.",
        ))
        .success();
}
