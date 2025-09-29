use crate::common::{TestContext, clients};
use icp_fs::fs::write;
use predicates::str::contains;

mod common;

#[tokio::test]
async fn token_balance() {
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

    let identity = clients::icp(&ctx, &project_dir).use_new_random_identity();
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
    clients::icp_ledger(&ctx)
        .mint_icp(identity, None, 123456780_u128)
        .await;

    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "icp", "balance"])
        .assert()
        .stdout(contains("Balance: 1.23456780 ICP"))
        .success();
}

#[tokio::test]
async fn token_transfer() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write(
        project_dir.join("icp.yaml"), // path
        "",                           // contents
    )
    .expect("failed to write project manifest");

    ctx.configure_icp_local_network_random_port(&project_dir);
    let _g = ctx.start_network_in(&project_dir);
    ctx.ping_until_healthy(&project_dir);

    let icp_client = clients::icp(&ctx, &project_dir);
    icp_client.create_identity("alice");
    icp_client.use_identity("alice");
    let alice_principal = icp_client.active_principal();
    icp_client.create_identity("bob");
    icp_client.use_identity("bob");
    let bob_principal = icp_client.active_principal();

    // Initial balance
    let icp_ledger = clients::icp_ledger(&ctx);
    icp_ledger
        .mint_icp(alice_principal, None, 1_000_000_000_u128)
        .await; // 10 ICP
    assert_eq!(
        icp_ledger.balance_of(alice_principal, None).await,
        1_000_000_000_u128
    );
    assert_eq!(icp_ledger.balance_of(bob_principal, None).await, 0_u128);

    // Simple ICP transfer
    icp_client.use_identity("alice");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["token", "transfer", "1.1", &bob_principal.to_string()])
        .assert()
        .stdout(contains(format!(
            "Transferred 1.10000000 ICP to {}",
            bob_principal
        )))
        .success();
    assert_eq!(
        icp_ledger.balance_of(alice_principal, None).await,
        889_990_000_u128
    );
    assert_eq!(
        icp_ledger.balance_of(bob_principal, None).await,
        110_000_000_u128
    );

    // Simple cycles transfer
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--icp", "5"])
        .assert()
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "token",
            "cycles",
            "transfer",
            "2",
            &bob_principal.to_string(),
        ])
        .assert()
        .stdout(contains(format!(
            "Transferred 2.000000000000 TCYCLES to {}",
            bob_principal
        )))
        .success();
    icp_client.use_identity("bob");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance"])
        .assert()
        .stdout(contains("Balance: 2.000000000000 TCYCLES"))
        .success();
}
