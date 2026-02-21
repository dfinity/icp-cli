use indoc::formatdoc;
use predicates::str::contains;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::IC_MAINNET_NETWORK_API_URL};

mod common;

#[tokio::test]
async fn cycles_balance() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
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

    // Wait for network
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Empty account has empty balance
    let identity = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .use_new_random_identity();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 0 cycles"))
        .success();

    // Mint ICP to cycles, specify ICP amount
    clients::ledger(&ctx)
        .acquire_icp(identity, None, 123456789_u64)
        .await;
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--icp",
            "1",
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains(
            "Minted 3_519_900_000_000 cycles to your account, new balance: 3_519_900_000_000 cycles.",
        ))
        .success();

    // Mint ICP to cycles, specify cycles amount
    let identity = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .use_new_random_identity();

    clients::ledger(&ctx)
        .acquire_icp(identity, None, 123456789_u64)
        .await;
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--cycles",
            "1T",
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains(
            "Minted 1_000_000_006_400 cycles to your account, new balance: 1_000_000_006_400 cycles.",
        ))
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--cycles",
            "1.5t",
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains(
            "Minted 1_500_000_025_600 cycles to your account, new balance: 2_500_000_032_000 cycles.",
        ))
        .success();
}

#[tokio::test]
async fn cycles_mint_with_explicit_network() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with network definition
    let pm = formatdoc! {r#"
        {NETWORK_RANDOM_PORT}
    "#};
    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Create identity and mint ICP
    let identity = clients::icp(&ctx, &project_dir, None).use_new_random_identity();
    clients::ledger(&ctx)
        .acquire_icp(identity, None, 123456789_u64)
        .await;

    // Run mint command with explicit --network flag
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--icp",
            "1",
            "--network",
            "random-network",
        ])
        .assert()
        .stdout(contains(
            "Minted 3_519_900_000_000 cycles to your account, new balance: 3_519_900_000_000 cycles.",
        ))
        .success();
}

#[tokio::test]
async fn cycles_mint_on_ic() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create identity
    clients::icp(&ctx, &project_dir, None).use_new_random_identity();

    // Run mint command with --network ic
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--icp", "1", "--network", "ic"])
        .assert()
        .stderr(contains(
            "Error: Insufficient funds: 1.00010000 ICP required, 0 ICP available.",
        ))
        .failure();

    // Run mint command with --network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "mint", "--icp", "1", "--network", "ic"])
        .assert()
        .stderr(contains(
            "Error: Insufficient funds: 1.00010000 ICP required, 0 ICP available.",
        ))
        .failure();
}

#[tokio::test]
async fn cycles_transfer() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("icp.yaml"),
        &formatdoc! {r#"
            {NETWORK_RANDOM_PORT}
            {ENVIRONMENT_RANDOM_PORT}
        "#},
    )
    .expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));

    icp_client.create_identity("alice");
    icp_client.use_identity("alice");
    let alice_principal = icp_client.active_principal();
    icp_client.create_identity("bob");
    icp_client.use_identity("bob");
    let bob_principal = icp_client.active_principal();

    // Mint ICP to alice and convert to cycles
    icp_client.use_identity("alice");
    clients::ledger(&ctx)
        .acquire_icp(alice_principal, None, 1_000_000_000_u128)
        .await;

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--icp",
            "5",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Transfer cycles from alice to bob
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "transfer",
            "2t",
            &bob_principal.to_string(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains(format!(
            "Transferred 2_000_000_000_000 cycles to {bob_principal}"
        )))
        .success();

    // Check bob's balance
    icp_client.use_identity("bob");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 2_000_000_000_000 cycles"))
        .success();
}

#[tokio::test]
async fn cycles_transfer_to_subaccount() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("icp.yaml"),
        &formatdoc! {r#"
            {NETWORK_RANDOM_PORT}
            {ENVIRONMENT_RANDOM_PORT}
        "#},
    )
    .expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));

    icp_client.create_identity("alice");
    icp_client.use_identity("alice");
    let alice_principal = icp_client.active_principal();
    icp_client.create_identity("bob");
    icp_client.use_identity("bob");
    let bob_principal = icp_client.active_principal();

    let subaccount_hex = format!("{:0>64}", "01");

    // Fund alice with cycles
    icp_client.use_identity("alice");
    clients::ledger(&ctx)
        .acquire_icp(alice_principal, None, 1_000_000_000_u128)
        .await;
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--icp",
            "5",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Transfer cycles to bob's subaccount using --to-subaccount
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "transfer",
            "1t",
            &bob_principal.to_string(),
            "--to-subaccount",
            &subaccount_hex,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Bob's default account should be empty
    icp_client.use_identity("bob");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 0 cycles"))
        .success();

    // Bob's subaccount should have the cycles
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "balance",
            "--subaccount",
            &subaccount_hex,
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains("Balance: 1_000_000_000_000 cycles"))
        .success();
}

#[tokio::test]
async fn cycles_mint_to_subaccount() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("icp.yaml"),
        &formatdoc! {r#"
            {NETWORK_RANDOM_PORT}
            {ENVIRONMENT_RANDOM_PORT}
        "#},
    )
    .expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    let identity = icp_client.use_new_random_identity();

    let icp_subaccount_hex = format!("{:0>64}", "01");
    let cycles_subaccount_hex = format!("{:0>64}", "02");

    let icp_subaccount: [u8; 32] = {
        let mut s = [0u8; 32];
        s[31] = 1;
        s
    };

    // Fund ICP into a subaccount
    clients::ledger(&ctx)
        .acquire_icp(identity, Some(icp_subaccount), 1_000_000_000_u128)
        .await;

    // Mint cycles from ICP subaccount into a cycles subaccount
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "mint",
            "--icp",
            "1",
            "--from-subaccount",
            &icp_subaccount_hex,
            "--to-subaccount",
            &cycles_subaccount_hex,
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains("Minted 3_519_900_000_000 cycles"))
        .success();

    // Default cycles account should be empty
    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "balance", "--environment", "random-environment"])
        .assert()
        .stdout(contains("Balance: 0 cycles"))
        .success();

    // Cycles subaccount should have the minted cycles
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "balance",
            "--subaccount",
            &cycles_subaccount_hex,
            "--environment",
            "random-environment",
        ])
        .assert()
        .stdout(contains("Balance: 3_519_900_000_000 cycles"))
        .success();
}
