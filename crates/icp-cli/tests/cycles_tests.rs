use indoc::formatdoc;
use predicates::prelude::PredicateBooleanExt;
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
        .stderr(contains("CyclePay sells cycles on mainnet: use -n ic"))
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
        .stderr(contains("CyclePay sells cycles on mainnet: use -n ic"))
        .failure();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["cycles", "buy", "--amount", "10", "--yes", "-e", "local"])
        .assert()
        .stderr(contains("is not running"))
        .stderr(contains("CyclePay sells cycles on mainnet: use -n ic"))
        .failure();

    ctx.icp()
        .args(["cycles", "buy", "--amount", "10", "--yes"])
        .assert()
        .stderr(contains("No project found"))
        .stderr(contains("CyclePay sells cycles on mainnet: use -n ic"))
        .failure();
}

/// A gateway id that cannot be a canister is refused in words, before any
/// network is asked about it; and a network that is not there points at
/// mainnet, outside a project as inside one.
#[test]
fn cycles_buy_refuses_a_gateway_that_cannot_be_one() {
    let ctx = TestContext::new();

    for (gateway, what) in [
        ("2vxsx-fae", "it is the anonymous principal"),
        ("aaaaa-aa", "it is the management canister"),
    ] {
        ctx.icp()
            .args([
                "cycles",
                "buy",
                "--amount",
                "10",
                "--yes",
                "--gateway",
                gateway,
                "-n",
                "ic",
            ])
            .assert()
            .stderr(contains(format!(
                "{gateway} cannot be a cycles gateway: {what}"
            )))
            .failure();
    }

    // No project, so no network called local: CyclePay is on mainnet.
    ctx.icp()
        .args(["cycles", "buy", "--amount", "10", "--yes", "-n", "local"])
        .assert()
        .stderr(contains(
            "Error: there is no network named 'local' outside a project.\n\
             CyclePay sells cycles on mainnet: use -n ic",
        ))
        .failure();
}

/// Deploy the stand-in gateway (`assets/cycles_gateway_canister`, a Motoko
/// actor serving the four methods the command uses) on a fresh local
/// network, and make a new identity, `buyer`, the default to buy with.
/// Returns the project directory, the gateway's canister id, and the
/// network's guard.
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

    client.create_identity("buyer");
    client.use_identity("buyer");
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
/// fails, and --json prints the order as made, then its end.
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
                "--yes",
                "--no-open",
                "--gateway",
                &gateway,
                "-n",
                "random-network",
            ])
            .args(args);
        cmd
    };

    let output = buy(&[]).output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(
            "https://buy.stripe.com/test_standin?client_reference_id=o_co-1-0000test&prefilled_amount=1000",
        ),
        "{output:?}"
    );
    // The error is wrapped to 80 columns: read it as prose.
    let said = String::from_utf8(output.stderr).unwrap().replace('\n', " ");
    assert!(
        said.contains("10.00 USD was refunded to your card"),
        "{said}"
    );
    assert!(said.contains("the cycles could not be delivered"), "{said}");

    let output = buy(&["--json"]).output().unwrap();
    assert!(!output.status.success(), "{output:?}");
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
    // Where the order stands is said once, as the error the command ends
    // with: still payable, where its cycles land, and the command that shows
    // them, on a line of its own, as the identity (the default, named) and
    // on the network the run used.
    assert!(
        said.ends_with(
            "Error: stopped waiting for order co-1-0000test.\n\
             It stays payable until 2100-01-01T00:00:00Z. Once it is paid, its cycles land on\n\
             your identity's cycles-ledger account. To see them:\n\
             \n    icp cycles balance --identity buyer -n random-network\n"
        ),
        "{said}"
    );
    assert_eq!(said.matches("stays payable").count(), 1, "{said}");
    assert!(!said.contains("interrupted while waiting"), "{said}");
}

/// With --json, a script is told how the wait ended whatever the end: here
/// an interrupt, as a last line on stdout after the order as made, which
/// names the gateway and the buyer.
#[cfg(unix)] // SIGINT, moc
#[tokio::test]
async fn cycles_buy_json_says_how_the_wait_ended() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

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
            "--json",
            "--gateway",
            &gateway,
            "-n",
            "random-network",
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
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();
    let status = child.wait().unwrap();
    stderr.read_to_string(&mut said).unwrap();
    let mut printed = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut printed)
        .unwrap();

    assert_eq!(status.code(), Some(1), "{said}");
    let objects: Vec<serde_json::Value> = printed
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(objects.len(), 2, "{printed}");
    assert_eq!(objects[0]["order_id"], "co-1-0000test");
    assert_eq!(objects[0]["gateway"], gateway.as_str());
    let buyer = objects[0]["buyer"].as_str().unwrap();
    candid::Principal::from_text(buyer).unwrap();
    assert_eq!(objects[1]["order_id"], "co-1-0000test");
    assert_eq!(objects[1]["status"], "awaiting_payment");
    assert_eq!(objects[1]["ended"], "interrupted");
    assert_eq!(objects[1]["expires_at"], "2100-01-01T00:00:00Z");
    // Every key is there, null where it does not apply.
    for key in ["cycles_delivered", "paid", "refunded", "detail", "balance"] {
        assert!(objects[1][key].is_null(), "{key}: {printed}");
    }
}

/// A payment by a method that has not settled is not called received; a
/// refund under way is not an end, and the command says how to see how it
/// ends. Off a terminal each step is a line.
#[cfg(unix)] // moc
#[tokio::test]
async fn cycles_buy_tells_a_pending_payment_and_a_refund_under_way() {
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
    settle(&ctx, &project_dir, &gateway, "pend", "()");
    while !said.contains("Payment pending") {
        assert_ne!(stderr.read_line(&mut said).unwrap(), 0, "{said}");
    }
    settle(&ctx, &project_dir, &gateway, "refunding", "()");
    stderr.read_to_string(&mut said).unwrap();
    let status = child.wait().unwrap();

    assert!(!status.success(), "{said}");
    assert!(
        said.contains("[order co-1-0000test] Payment pending; waiting for it to settle..."),
        "{said}"
    );
    assert!(!said.contains("Payment received"), "{said}");
    let prose = said.replace('\n', " ");
    assert!(
        prose.contains("A refund to your card is under way"),
        "{said}"
    );
    assert!(prose.contains("owed a refund"), "{said}");
    // The command that reads the order back, on a line of its own, as the
    // identity that made the order, which alone can read it.
    assert!(
        said.contains(&format!(
            "\n    icp canister call {gateway} order_status --query --identity buyer \
             -n random-network '(\"co-1-0000test\")'\n"
        )),
        "{said}"
    );
    // Not CyclePay's gateway: nothing of CyclePay's is said of it.
    assert!(!said.contains("CyclePay"), "{said}");
}

/// A gateway that stops answering is said to while the wait goes on trying,
/// and so is its answering again; the wait then ends as it would have.
#[cfg(unix)] // moc
#[tokio::test]
async fn cycles_buy_says_when_the_gateway_stops_answering() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let ctx = TestContext::new();
    let (project_dir, gateway, _g) = stand_in_gateway(&ctx).await;
    let canister = |verb: &str| {
        ctx.icp()
            .current_dir(&project_dir)
            .args([
                "canister",
                verb,
                "gateway",
                "--environment",
                "random-environment",
                // The stand-in's controller.
                "--identity",
                "anonymous",
            ])
            .assert()
            .success();
    };

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
    // A stopped canister refuses every poll, as CyclePay's backend does for
    // a few minutes during an upgrade.
    canister("stop");
    while !said.contains("No answer from the gateway") {
        assert_ne!(stderr.read_line(&mut said).unwrap(), 0, "{said}");
    }
    canister("start");
    while !said.contains("answering again") {
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
    assert!(status.success(), "{said}");
    assert!(
        said.contains("[order co-1-0000test] No answer from the gateway; retrying..."),
        "{said}"
    );
    assert!(
        said.contains("[order co-1-0000test] The gateway is answering again."),
        "{said}"
    );
}

/// A canister that serves no gateway, or none at all, is said to be so in
/// words, not in the replica's or the HTTP gateway's.
#[cfg(unix)] // moc
#[tokio::test]
async fn cycles_buy_names_a_canister_that_is_no_gateway() {
    let ctx = TestContext::new();
    let (project_dir, _gateway, _g) = stand_in_gateway(&ctx).await;
    let buy = |gateway: &str| {
        let mut cmd = ctx.icp();
        cmd.current_dir(&project_dir).args([
            "cycles",
            "buy",
            "--amount",
            "10",
            "--yes",
            "--gateway",
            gateway,
            "-n",
            "random-network",
        ]);
        cmd
    };

    // Wrapped to 80 columns: read as prose.
    let said = |gateway: &str| {
        let output = buy(gateway).output().unwrap();
        assert!(!output.status.success(), "{output:?}");
        String::from_utf8(output.stderr).unwrap().replace('\n', " ")
    };

    // The ICP ledger, which every local network has.
    let ledger = said("ryjl3-tyaaa-aaaaa-aaaba-cai");
    assert!(
        ledger.contains(
            "canister ryjl3-tyaaa-aaaaa-aaaba-cai is not a cycles gateway: it does not serve \
             get_card_terms, the method"
        ),
        "{ledger}"
    );
    assert!(!ledger.contains("Caused by"), "{ledger}");

    // A canister id no canister on this network has.
    let unused = said("ht2ng-gaaaa-aaaac-p77ya-cai");
    assert!(
        unused.contains(
            "the cycles gateway canister ht2ng-gaaaa-aaaac-p77ya-cai does not exist on this \
             network"
        ),
        "{unused}"
    );
    assert!(!unused.contains("HTTP"), "{unused}");
}

/// With --no-wait the command returns once the order is made, and says
/// where its cycles will show, with the command that shows them on a line
/// of its own.
#[cfg(unix)] // moc
#[tokio::test]
async fn cycles_buy_no_wait_says_where_the_cycles_will_show() {
    let ctx = TestContext::new();
    let (project_dir, gateway, _g) = stand_in_gateway(&ctx).await;

    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "buy",
            "--amount",
            "10",
            "--yes",
            "--no-wait",
            "--gateway",
            &gateway,
            "-n",
            "random-network",
        ])
        .output()
        .expect("failed to run icp cycles buy");
    let said = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{said}");
    assert!(
        said.ends_with(
            "Not waiting. Once the order is paid, its cycles land on your identity's\n\
             cycles-ledger account. To see them:\n\
             \n    icp cycles balance --identity buyer -n random-network\n"
        ),
        "{said}"
    );
}

/// A wait through payment to delivery: once the stand-in gateway is paid
/// (its `pay` update), the next poll finds the order delivered, and the
/// command reports the gateway's delivery to where the order says it went,
/// without calling a gateway that is not CyclePay's by its name, and
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
            "The gateway reports 6.938T cycles delivered to canister rrkah-fqaaa-aaaaa-aaaaq-cai"
        ),
        "{printed}"
    );
    // Not the buyer's account, so no balance of the buyer's is read.
    assert!(!printed.contains("Balance"), "{printed}");
}

/// Short of cycles on a local network, `icp deploy` and `icp canister create`
/// say how many are missing, and give a transfer from the anonymous identity,
/// which the network funds, and the conversion of ICP, as the identity and on
/// the environment or network they ran with. They never mention buying them
/// with a card, which is for mainnet alone. `--json` adds nothing to stdout.
/// Off a terminal, as here, the message is plain text.
///
/// The figures are the ledger's own: one cycle short, the commands printed
/// ask for that cycle and a margin, and once either has been run as printed
/// the creation goes through. A deploy that can pay for some of its
/// canisters is told what the rest need, and that fewer cycles would do. The
/// anonymous identity is told to convert ICP. Creating with ICP runs short
/// of ICP, not cycles, and gets no cycles hint.
#[tokio::test]
async fn running_short_of_cycles_says_how_many_and_how_to_get_them() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: one
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
          - name: two
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
    let client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));

    // A new identity, with no cycles at all, as any made after the network
    // started is.
    client.create_identity("short");
    let short = client.get_principal("short").to_text();

    // Two canisters at the default 2T each, and the ledger's fee on each.
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--environment",
            "random-environment",
            "--identity",
            "short",
        ])
        .output()
        .expect("failed to run icp deploy");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.ends_with(&format!(
            "Error: not enough cycles to create 2 canisters.\n\
             It takes 4.0002T cycles, fees included, and identity short has 0.\n\
             Get 4.03T more, from the anonymous identity, which a local network funds, or\n\
             from ICP, then run the deploy again:\n\n    \
             icp cycles transfer 4.03T {short} --identity anonymous -e random-environment\n    \
             icp cycles mint --cycles 4.03T --identity short -e random-environment\n"
        )),
        "{stderr}"
    );
    assert!(
        !stderr.contains("card") && !stderr.contains('\x1b'),
        "{stderr}"
    );

    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--environment",
            "random-environment",
            "--identity",
            "short",
            "--json",
        ])
        .output()
        .expect("failed to run icp deploy");
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(
            "    icp cycles mint --cycles 4.03T --identity short -e random-environment\n"
        ),
        "{stderr}"
    );

    // One cycle short of one canister, on a network named rather than an
    // environment: funded with cycles from the default identity, which
    // pays the transfer's fee, and ICP to convert.
    client.create_identity("nearly");
    let nearly = client.get_principal("nearly").to_text();
    for (what, amount) in [("cycles", "2000099999999"), ("token", "100")] {
        ctx.icp()
            .current_dir(&project_dir)
            .args([
                what,
                "transfer",
                amount,
                &nearly,
                "--environment",
                "random-environment",
            ])
            .assert()
            .success();
    }
    let create = || {
        let mut command = ctx.icp();
        command.current_dir(&project_dir).args([
            "canister",
            "create",
            "--detached",
            "--identity",
            "nearly",
            "--network",
            "random-network",
        ]);
        command
    };
    // The command on the line that starts `    icp cycles {what}`, run as
    // printed.
    let run_printed = |stderr: &str, what: &str| {
        let printed = stderr
            .lines()
            .find_map(|line| line.strip_prefix(&format!("    icp cycles {what} ")))
            .unwrap_or_else(|| panic!("no icp cycles {what} in {stderr}"));
        ctx.icp()
            .current_dir(&project_dir)
            .args(["cycles", what])
            .args(printed.split(' '))
            .assert()
            .success();
    };
    let output = create()
        .output()
        .expect("failed to run icp canister create");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.ends_with(&format!(
            "Error: not enough cycles to create the canister.\n\
             It takes 2.0001T cycles, fees included, and identity nearly has 2.000099999999T.\n\
             Get 1.01M more, from the anonymous identity, which a local network funds, or\n\
             from ICP, then create the canister again:\n\n    \
             icp cycles transfer 1.01M {nearly} --identity anonymous -n random-network\n    \
             icp cycles mint --cycles 1.01M --identity nearly -n random-network\n\n\
             You can also give the canister fewer cycles with --cycles,\n\
             at least the 0.5T a creation costs.\n"
        )),
        "{stderr}"
    );
    // The transfer, run as printed, and the creation goes through.
    run_printed(&stderr, "transfer");
    create().assert().success();
    // Short again, by what that took; the conversion, run as printed, and it
    // goes through.
    let output = create()
        .output()
        .expect("failed to run icp canister create");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr
            .contains("\n    icp cycles mint --cycles 2.02T --identity nearly -n random-network\n"),
        "{stderr}"
    );
    run_printed(&stderr, "mint");
    create().assert().success();

    // Enough for one of the two canisters: one is created, and the error
    // covers the other, from what the first left.
    client.create_identity("half");
    let half = client.get_principal("half").to_text();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "cycles",
            "transfer",
            "3t",
            &half,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--environment",
            "random-environment",
            "--identity",
            "half",
        ])
        .assert()
        .failure()
        .stderr(contains(
            "Error: not enough cycles to create 1 canister.\n\
             It takes 2.0001T cycles, fees included, and identity half has 999.9B.\n\
             Get 1.01T more, from the anonymous identity, which a local network funds, or\n\
             from ICP, then run the deploy again:\n\n",
        ))
        .stderr(contains(
            "\n\nYou can also give each canister fewer cycles with --cycles,\n\
             at least the 0.5T a creation costs.\n",
        ));

    // The anonymous identity, the default here, holds more cycles on a local
    // network than a canister can be given, but not as many as asked for:
    // the ordinary hint, converting ICP, as it was selected; no transfer from
    // itself, and no word of making an identity.
    let anonymous = |identity: &[&str]| {
        let mut command = ctx.icp();
        command
            .current_dir(&project_dir)
            .args([
                "canister",
                "create",
                "--detached",
                "--network",
                "random-network",
                "--cycles",
                "300000000000000000000000000T",
            ])
            .args(identity);
        command
    };
    for (identity, flag) in [
        (&[][..], ""),
        (&["--identity", "anonymous"][..], " --identity anonymous"),
    ] {
        anonymous(identity)
            .assert()
            .failure()
            .stderr(contains("\nand the anonymous identity has "))
            .stderr(contains(
                " more from ICP, then create the canister again:\n\n    \
                 icp cycles mint --cycles ",
            ))
            .stderr(contains(format!(
                "T{flag} -n random-network\n\n\
                 You can also give the canister fewer cycles with --cycles,\n\
                 at least the 0.5T a creation costs.\n"
            )))
            .stderr(contains("icp identity").not())
            .stderr(contains("icp cycles transfer").not());
    }

    // Creating with ICP the identity does not have: the ICP ledger's
    // refusal, as before, and no word of cycles to get.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--detached",
            "--with-icp",
            "1",
            "--identity",
            "short",
            "--network",
            "random-network",
        ])
        .assert()
        .failure()
        .stderr(contains("InsufficientFunds"))
        .stderr(contains("more cycles").not())
        .stderr(contains("icp cycles").not());
}
