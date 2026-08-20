//! `icp message send`: submitting a message that was signed elsewhere.
//!
//! The round-trip tests run the whole two-machine workflow against a local
//! network — sign with no network in reach, then submit — since that pairing is
//! the only thing that proves either half works.

use indoc::formatdoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::Value;

use crate::common::{ChildGuard, ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext};
use icp::fs::write_string;
use icp::prelude::*;

mod common;

const GREET_DID: &str = r#"service : { "greet" : (text) -> (text) query }"#;

/// A project with a canister deployed to a running local network.
///
/// The returned guard owns the network process: the caller has to hold it for
/// as long as it needs the network, and dropping it shuts the network down.
async fn deployed(ctx: &TestContext) -> (PathBuf, ChildGuard) {
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
    write_string(&project_dir.join("icp.yaml"), &pm).expect("write manifest");

    let guard = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    (project_dir, guard)
}

/// The whole point: sign on one machine, submit from another, get the reply.
#[tokio::test]
async fn round_trip_update() {
    let ctx = TestContext::new();
    let (project_dir, _network) = deployed(&ctx).await;
    let msg = project_dir.join("update.json");

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--sign-only",
            msg.as_str(),
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    // Submitting needs no identity, no project and no interface of its own —
    // only the file. Run it from outside the project to prove that.
    ctx.icp()
        .args(["message", "send", msg.as_str(), "--yes"])
        .assert()
        .success()
        .stdout(contains("Hello, world!"))
        .stderr(contains("Submittable for another"));
}

/// A signed query takes the other endpoint and carries no status check.
#[tokio::test]
async fn round_trip_query() {
    let ctx = TestContext::new();
    let (project_dir, _network) = deployed(&ctx).await;
    let msg = project_dir.join("query.json");

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--query",
            "--sign-only",
            msg.as_str(),
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    ctx.icp()
        .args(["message", "send", msg.as_str(), "--yes"])
        .assert()
        .success()
        .stdout(contains("Hello, world!"));
}

/// Re-running `send` on the same file is the documented recovery when a
/// submission fails after the message may already have gone out. It has to be
/// accepted and yield the same answer — that is what the advice depends on.
/// (It does not, and cannot from here, observe how many times the call ran; the
/// IC's de-duplication of an identical request id is what guarantees that.)
#[tokio::test]
async fn resending_the_same_file_is_accepted() {
    let ctx = TestContext::new();
    let (project_dir, _network) = deployed(&ctx).await;
    let msg = project_dir.join("resend.json");

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--sign-only",
            msg.as_str(),
            "my-canister",
            "greet",
            "(\"once\")",
        ])
        .assert()
        .success();

    for attempt in 0..2 {
        ctx.icp()
            .args(["message", "send", msg.as_str(), "--yes"])
            .assert()
            .success()
            .stdout(contains("Hello, once!"));
        eprintln!("submission {} accepted", attempt + 1);
    }
}

/// `--dry-run` inspects the file and stops. It stays entirely offline, which is
/// what makes it safe to point at a message you have not decided to send yet —
/// the network here does not exist at all.
#[test]
fn dry_run_inspects_without_sending() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("write candid");
    let msg = ctx.home_path().join("message.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            "http://127.0.0.1:1",
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            msg.as_str(),
            "ryjl3-tyaaa-aaaaa-aaaba-cai",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    ctx.icp()
        .args(["message", "send", msg.as_str(), "--dry-run"])
        .assert()
        .success()
        // The argument is decoded with the interface the signer embedded, so the
        // summary is reviewable rather than a hex blob.
        .stderr(contains("(\"world\")"))
        .stderr(contains("greet (update)"))
        .stderr(contains(
            "Destination: canister ryjl3-tyaaa-aaaaa-aaaba-cai",
        ))
        .stderr(contains("Not submitted"));
}

/// Where a message goes is the file's business. A courier who genuinely has to
/// redirect one edits the file — `network` is unauthenticated either way, so that
/// is inside the trust model. Nothing outside the file can change it, and in
/// particular `ICP_NETWORK` cannot: a stray shell variable silently sending a
/// signed message to mainnet is exactly the accident there is no flag for.
#[test]
fn only_the_file_says_where_to_submit() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("write candid");
    let msg = ctx.home_path().join("redirect.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            "http://127.0.0.1:9999",
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            msg.as_str(),
            "ryjl3-tyaaa-aaaaa-aaaba-cai",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    // An edited network is honoured, and the message still validates: the
    // envelope is signed and carries no URL, so this cannot change what executes.
    let mut file: Value =
        serde_json::from_str(&icp::fs::read_to_string(&msg).expect("read")).expect("JSON");
    file["network"]["url"] = Value::String("http://127.0.0.1:1234/".into());
    write_string(
        &msg,
        &serde_json::to_string_pretty(&file).expect("serialize"),
    )
    .expect("write");

    ctx.icp()
        .args(["message", "send", msg.as_str(), "--dry-run"])
        .env("ICP_NETWORK", "ic")
        .assert()
        .success()
        .stderr(contains("Network:     http://127.0.0.1:1234/"))
        // Not mainnet, which is where `ICP_NETWORK=ic` would have pointed it.
        .stderr(contains("icp-api.io").not());
}

/// The summary is display-only, so a file whose summary disagrees with its
/// signed envelope is refused rather than quietly believed either way.
#[test]
fn tampered_file_is_refused() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("write candid");
    let msg = ctx.home_path().join("tampered.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            "http://127.0.0.1:1",
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            msg.as_str(),
            "ryjl3-tyaaa-aaaaa-aaaba-cai",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    let mut file: Value =
        serde_json::from_str(&icp::fs::read_to_string(&msg).expect("read")).expect("JSON");
    file["summary"]["method"] = Value::String("transfer".into());
    write_string(
        &msg,
        &serde_json::to_string_pretty(&file).expect("serialize"),
    )
    .expect("write");

    ctx.icp()
        .args(["message", "send", msg.as_str(), "--yes"])
        .assert()
        .failure()
        .stderr(contains("summary does not match the signed request"));
}

/// A message whose window has not opened is refused with the opening time, so
/// the operator knows to wait rather than to re-sign. Refused before anything
/// touches the network: the URL here is unreachable.
#[test]
fn not_yet_valid_file_is_refused() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("write candid");
    let msg = ctx.home_path().join("later.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            "http://127.0.0.1:1",
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            msg.as_str(),
            "--valid-from",
            "1h",
            "ryjl3-tyaaa-aaaaa-aaaba-cai",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    ctx.icp()
        .args(["message", "send", msg.as_str(), "--yes"])
        .assert()
        .failure()
        .stderr(contains("Not yet submittable"))
        .stderr(contains("does not open until").and(contains("Run this again then")));
}

/// An expired message is refused with the time the window closed. Built here
/// rather than signed, because `--sign-only` will not produce a window that has
/// already passed — a query, since it needs no status check to keep in step.
#[test]
fn expired_file_is_refused() {
    use ic_agent::{Agent, identity::AnonymousIdentity};
    use icp::signed_message::{
        CallType, Destination, Network, Request, SUBMISSION_WINDOW, SignedMessage, Summary,
        format_timestamp,
    };
    use time::OffsetDateTime;

    let ctx = TestContext::new();
    let canister = candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("principal");

    // A window that closed ten minutes ago.
    let valid_until = (OffsetDateTime::now_utc() - time::Duration::minutes(10))
        .replace_nanosecond(0)
        .and_then(|t| t.replace_second(0))
        .expect("0 is a valid second and nanosecond");
    let valid_from = valid_until - SUBMISSION_WINDOW;

    let agent = Agent::builder()
        .with_url("http://127.0.0.1:1")
        .with_identity(AnonymousIdentity)
        .build()
        .expect("building an agent makes no request");
    let signed = agent
        .query(&canister, "greet")
        .with_arg(b"arg".to_vec())
        .expire_at(valid_until)
        .sign()
        .expect("signing makes no request");

    let message = SignedMessage {
        format: icp::signed_message::FORMAT.to_string(),
        version: icp::signed_message::VERSION,
        request: Request {
            call_type: CallType::Query,
            envelope: signed.signed_query,
            request_id: None,
            status_check: None,
        },
        network: Network {
            url: "http://127.0.0.1:1".parse().expect("url"),
            root_key: icp::network::RootKeySpec::Mainnet,
        },
        destination: Destination::Canister(canister),
        candid: None,
        summary: Summary {
            sender: signed.sender,
            canister_id: canister,
            method: "greet".to_string(),
            arg: b"arg".to_vec(),
            signed_at: format_timestamp(valid_from),
            valid_from: format_timestamp(valid_from),
            valid_until: format_timestamp(valid_until),
        },
    };

    let msg = ctx.home_path().join("expired.json");
    message.save(&msg).expect("save");

    ctx.icp()
        .args(["message", "send", msg.as_str(), "--yes"])
        .assert()
        .failure()
        .stderr(contains("Expired"))
        .stderr(contains(&format_timestamp(valid_until)[..]))
        .stderr(contains("signed again"));
}
