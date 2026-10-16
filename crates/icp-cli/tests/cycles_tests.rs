use indoc::formatdoc;
use predicates::str::contains;

#[cfg(unix)] // the stand-in gateway needs moc
use crate::common::ChildGuard;
use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp_project::fs::write_string;
#[cfg(unix)]
use icp_project::prelude::{PathBuf, TRILLION};

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
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "balance",
            "--of-principal",
            &bob_principal.to_string(),
            "--environment",
            "random-environment",
        ])
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

/// The two ways into `cycles buy` are exclusive, and so are its two
/// machine-readable outputs; none of this needs a network.
#[test]
fn cycles_buy_rejects_conflicting_arguments() {
    use predicates::prelude::PredicateBooleanExt;

    let ctx = TestContext::new();

    ctx.icp()
        .args(["cycles", "buy", "--amount", "10", "--json", "--quiet"])
        .assert()
        .stderr(contains("cannot be used with"))
        .failure();

    // Neither: the one of the two that is required is named, not --amount.
    ctx.icp()
        .args(["cycles", "buy"])
        .assert()
        .stderr(contains("required arguments were not provided"))
        .stderr(contains("<--amount <AMOUNT>|--cycles <CYCLES>>"))
        .failure();

    // A negative value of either is refused in the same words and with the
    // same status, the form with a suffix included, rather than read as a
    // flag nobody passed.
    for (flag, value) in [("--amount", "-1"), ("--cycles", "-1"), ("--cycles", "-5t")] {
        ctx.icp()
            .args(["cycles", "buy", flag, value])
            .assert()
            .stderr(contains(format!("invalid value '{value}' for '{flag} <")))
            .stderr(contains("cannot be negative: give the"))
            .stderr(contains("unexpected argument").not())
            .code(2);
    }

    ctx.icp()
        .args(["cycles", "buy", "--amount", "10.005"])
        .assert()
        .stderr(contains("more than 2 decimal places"))
        .failure();

    ctx.icp()
        .args(["cycles", "buy", "--amount", "0"])
        .assert()
        .stderr(contains("greater than zero"))
        .failure();

    ctx.icp()
        .args(["cycles", "buy", "--amount", "10", "--cycles", "5t"])
        .assert()
        .stderr(contains("cannot be used with"))
        .failure();

    ctx.icp()
        .args(["cycles", "buy", "--cycles", "0"])
        .assert()
        .stderr(contains("greater than zero"))
        .failure();
}

/// The anonymous identity cannot own an order, so the command refuses before
/// touching the network.
#[test]
fn cycles_buy_refuses_the_anonymous_identity() {
    let ctx = TestContext::new();

    ctx.icp()
        .args([
            "cycles",
            "buy",
            "--amount",
            "10",
            "--yes",
            "--identity",
            "anonymous",
            "-n",
            "ic",
        ])
        .assert()
        .stderr(contains("anonymous identity"))
        .failure();
}

/// CyclePay's gateway is on mainnet only: on a local network the command
/// says so before asking the network for anything, and points at `-n ic`.
#[tokio::test]
async fn cycles_buy_on_a_local_network_points_at_mainnet() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = formatdoc! {r#"
        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .use_new_random_identity();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "buy",
            "--amount",
            "10",
            "--yes",
            "--environment",
            "random-environment",
        ])
        .assert()
        .stderr(contains("this network is not mainnet"))
        .stderr(contains("CyclePay sells cycles on mainnet: use `-n ic`"))
        .failure();
}

/// With no network reachable, whether none was named outside a project or
/// the one chosen is a local one that is not running, the command points at
/// `-n ic`: CyclePay is on mainnet only.
#[test]
fn cycles_buy_with_no_network_reachable_points_at_mainnet() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    write_string(&project_dir.join("icp.yaml"), "").expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "buy", "--amount", "10", "--yes"])
        .assert()
        .stderr(contains("is not running"))
        .stderr(contains("CyclePay sells cycles on mainnet: use `-n ic`"))
        .failure();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "buy", "--amount", "10", "--yes", "-e", "local"])
        .assert()
        .stderr(contains("is not running"))
        .stderr(contains("CyclePay sells cycles on mainnet: use `-n ic`"))
        .failure();

    ctx.icp()
        .args(["cycles", "buy", "--amount", "10", "--yes"])
        .assert()
        .stderr(contains("No project found"))
        .stderr(contains("CyclePay sells cycles on mainnet: use `-n ic`"))
        .failure();
}

/// Deploy the stand-in gateway (`assets/cycles_gateway_canister`, a Motoko
/// actor serving the four methods the command uses) on a fresh local
/// network, and switch to a random identity to buy with. Returns the project
/// directory, the gateway's canister id, and the network's guard.
#[cfg(unix)] // moc
async fn stand_in_gateway(ctx: &TestContext) -> (PathBuf, String, ChildGuard) {
    let project_dir = ctx.create_project_dir("icp");
    ctx.copy_asset_dir("cycles_gateway_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: gateway
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let guard = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let client = clients::icp(ctx, &project_dir, Some("random-environment".to_string()));
    client.mint_cycles(10 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "gateway",
            "--environment",
            "random-environment",
            "--id-only",
        ])
        .output()
        .expect("failed to read the gateway's canister id");
    assert!(output.status.success(), "{output:?}");
    let gateway = String::from_utf8(output.stdout).unwrap().trim().to_owned();

    client.use_new_random_identity();
    (project_dir, gateway, guard)
}

/// Tell the stand-in gateway where its order stands: `pay` with the canister
/// the cycles went to, or `refund`.
#[cfg(unix)] // moc
fn settle(ctx: &TestContext, project_dir: &PathBuf, gateway: &str, how: &str, arg: &str) {
    ctx.icp()
        .current_dir(project_dir)
        .args([
            "canister",
            "call",
            gateway,
            how,
            arg,
            "--identity",
            "anonymous",
            "--network",
            "random-network",
        ])
        .assert()
        .success();
}

/// An order the gateway cannot deliver is refunded: the command says so and
/// fails, and --json prints the order as made, then its end. A --json run
/// implies --yes: off a terminal, as here, it neither asks nor refuses, and
/// the quote stays off stderr.
#[cfg(unix)] // moc
#[tokio::test]
async fn cycles_buy_reports_a_refunded_order() {
    let ctx = TestContext::new();
    let (project_dir, gateway, _g) = stand_in_gateway(&ctx).await;
    // The stand-in's order was paid, could not be delivered, and was refunded.
    settle(&ctx, &project_dir, &gateway, "refund", "()");

    let buy = |args: &[&str]| {
        let mut cmd = ctx.icp();
        cmd.current_dir(&project_dir)
            .args([
                "cycles",
                "buy",
                "--amount",
                "10",
                "--no-open",
                "--gateway",
                &gateway,
                "-n",
                "random-network",
            ])
            .args(args);
        cmd
    };

    buy(&["--yes"])
        .assert()
        .stdout(contains(
            "https://buy.stripe.com/test_standin?client_reference_id=o_co-1-0000test&prefilled_amount=1000",
        ))
        .stderr(contains("10.00 USD was refunded to your card"))
        .stderr(contains("the cycles could not be delivered"))
        .failure();

    let output = buy(&["--json"]).output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    let said = String::from_utf8(output.stderr).unwrap();
    assert!(!said.contains("Refusing to create an order"), "{said}");
    assert!(!said.contains("Price:"), "{said}");
    let printed = String::from_utf8(output.stdout).unwrap();
    let objects: Vec<serde_json::Value> = printed
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(objects.len(), 2, "{printed}");
    assert_eq!(objects[0]["order_id"], "co-1-0000test");
    assert_eq!(objects[0]["amount"]["value"], "10.00");
    assert_eq!(objects[0]["fee"]["value"], "0.59");
    assert!(
        objects[0]["checkout_url"]
            .as_str()
            .unwrap()
            .starts_with("https://buy.stripe.com/test_standin?"),
        "{printed}"
    );
    assert_eq!(objects[1]["order_id"], "co-1-0000test");
    assert_eq!(objects[1]["status"], "refunded");
    assert_eq!(objects[1]["refunded"]["value"], "10.00");
}

/// Ctrl-C while waiting stops the wait, not the order: the command says the
/// order stays payable and exits 1, as icp-cli's other interrupted waits do.
/// Off a terminal, as here, the wait is told in lines. The link handed out
/// is the stand-in's own: nothing of CyclePay's is built in.
#[cfg(unix)] // SIGINT, moc
#[tokio::test]
async fn cycles_buy_ctrl_c_stops_the_wait_not_the_order() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let ctx = TestContext::new();
    // The stand-in's order awaits payment until told otherwise.
    let (project_dir, gateway, _g) = stand_in_gateway(&ctx).await;

    let mut child = ctx
        .icp_std()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "buy",
            "--amount",
            "10",
            "--yes",
            "--gateway",
            &gateway,
            "-n",
            "random-network",
            "--no-open",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn icp cycles buy");
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut said = String::new();
    // Off a terminal the wait says it has begun, and that the order does not
    // need it.
    while !said.contains("Waiting for payment") {
        assert_ne!(stderr.read_line(&mut said).unwrap(), 0, "{said}");
    }
    assert!(said.contains("carries on without it"), "{said}");
    std::thread::sleep(std::time::Duration::from_secs(1));
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();
    let status = child.wait().unwrap();
    std::io::Read::read_to_string(&mut stderr, &mut said).unwrap();

    let mut printed = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut printed).unwrap();

    assert_eq!(status.code(), Some(1), "{said}");
    assert_eq!(
        printed.trim(),
        "https://buy.stripe.com/test_standin?client_reference_id=o_co-1-0000test&prefilled_amount=1000",
    );
    assert!(said.contains("stays payable"), "{said}");
    assert!(said.contains("icp cycles balance"), "{said}");
    assert!(
        said.contains("interrupted while waiting for order co-1-0000test"),
        "{said}"
    );
}

/// A wait through payment to delivery: once the stand-in gateway is paid
/// (its `pay` update), the next poll finds the order delivered, and the
/// command reports CyclePay's delivery to where the order says it went and
/// succeeds.
#[cfg(unix)] // moc
#[tokio::test]
async fn cycles_buy_waits_through_payment_to_delivery() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let ctx = TestContext::new();
    let (project_dir, gateway, _g) = stand_in_gateway(&ctx).await;

    let mut child = ctx
        .icp_std()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "buy",
            "--amount",
            "10",
            "--yes",
            "--gateway",
            &gateway,
            "-n",
            "random-network",
            "--no-open",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn icp cycles buy");
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut said = String::new();
    while !said.contains("Waiting for payment") {
        assert_ne!(stderr.read_line(&mut said).unwrap(), 0, "{said}");
    }

    settle(
        &ctx,
        &project_dir,
        &gateway,
        "pay",
        "(principal \"rrkah-fqaaa-aaaaa-aaaaq-cai\")",
    );

    stderr.read_to_string(&mut said).unwrap();
    let status = child.wait().unwrap();
    let mut printed = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut printed)
        .unwrap();
    assert!(status.success(), "{said}");
    assert!(
        printed.contains(
            "CyclePay reports 6.938T cycles delivered to canister rrkah-fqaaa-aaaaa-aaaaq-cai"
        ),
        "{printed}"
    );
    // Not the buyer's account, so no balance of the buyer's is read.
    assert!(!printed.contains("Balance"), "{printed}");
}
