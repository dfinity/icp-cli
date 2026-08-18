//! `icp canister call --sign-only`: composing and signing a call on a machine
//! that holds the key and has no network.
//!
//! None of these tests start a network, and the URLs they name are deliberately
//! unreachable — that is the point of the feature, and a test that quietly
//! depended on a reachable network would stop testing it.

use indoc::formatdoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::Value;

use crate::common::TestContext;
use icp::fs::write_string;
use icp::prelude::*;

mod common;

/// A canister id to sign calls to. Nothing is ever sent to it.
const TARGET: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

/// A network that cannot be reached, with a root key that needs no round trip
/// to resolve.
const UNREACHABLE: &str = "http://127.0.0.1:1";

const GREET_DID: &str = r#"service : { "greet" : (text) -> (text) query }"#;

/// The `ingress_expiry` of a base64-encoded, CBOR-encoded authentication envelope.
fn envelope_expiry(encoded: &str) -> u64 {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let cbor = STANDARD.decode(encoded).expect("envelope must be base64");
    let envelope: ic_agent::agent::Envelope =
        serde_cbor::from_slice(&cbor).expect("envelope must be CBOR");
    envelope.content.ingress_expiry()
}

fn read_message(path: &Path) -> Value {
    let text = icp::fs::read_to_string(path).expect("the message file must exist");
    serde_json::from_str(&text).expect("a signed message must be JSON a human can read")
}

/// Signing needs no project, no network, and no canister-name resolution:
/// a principal, an interface, and a key are enough.
#[test]
fn signs_an_update_with_no_network_and_no_project() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("failed to write candid file");
    let out = ctx.home_path().join("message.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            UNREACHABLE,
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            out.as_str(),
            TARGET,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stderr(contains("five-minute window"));

    let message = read_message(&out);
    assert_eq!(message["format"], "icp-signed-message");
    assert_eq!(message["version"], 1);
    assert_eq!(message["request"]["type"], "update");
    assert_eq!(message["network"]["url"], format!("{UNREACHABLE}/"));
    assert_eq!(message["network"]["root_key"], "mainnet");
    // Tagged, so the submitting machine knows which endpoint shape routes it.
    assert_eq!(message["destination"]["canister"], TARGET);
    assert_eq!(message["summary"]["canister_id"], TARGET);
    assert_eq!(message["summary"]["method"], "greet");
    // The interface travels with the message: the submitting machine has no
    // project to resolve one from.
    assert_eq!(message["candid"], GREET_DID);

    // An update carries what it takes to await the outcome with no key.
    let request_id = message["request"]["request_id"]
        .as_str()
        .expect("an update records its request id");
    assert_eq!(
        request_id.len(),
        64,
        "request id is hex-encoded: {request_id}"
    );

    // Both envelopes must expire at the same instant. The window is
    // `[expiry - 5min, expiry]`, so a status check with an expiry of its own
    // would be waiting on the call from a different window.
    let call_expiry = envelope_expiry(
        message["request"]["envelope"]
            .as_str()
            .expect("the envelope is base64 text"),
    );
    let status_expiry = envelope_expiry(
        message["request"]["status_check"]
            .as_str()
            .expect("an update records a pre-signed status check"),
    );
    assert_eq!(
        call_expiry, status_expiry,
        "the call and its status check must share one submission window"
    );
}

/// The window is five minutes wide, always; `--valid-from` only places it.
#[test]
fn valid_from_places_a_five_minute_window() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("failed to write candid file");

    let sign = |out: &Path, valid_from: Option<&str>| {
        let mut cmd = ctx.icp();
        cmd.args([
            "canister",
            "call",
            "--network",
            UNREACHABLE,
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            out.as_str(),
            TARGET,
            "greet",
            "(\"world\")",
        ]);
        if let Some(valid_from) = valid_from {
            cmd.args(["--valid-from", valid_from]);
        }
        cmd.assert().success();
        read_message(out)
    };

    let window = |message: &Value| {
        let parse = |field: &str| {
            time::OffsetDateTime::parse(
                message["summary"][field].as_str().expect("a timestamp"),
                &time::format_description::well_known::Rfc3339,
            )
            .expect("timestamps are RFC 3339")
        };
        (parse("valid_from"), parse("valid_until"))
    };

    // Default: the window is open now.
    let now_message = sign(&ctx.home_path().join("now.json"), None);
    let (from, until) = window(&now_message);
    assert_eq!(until - from, time::Duration::minutes(5));
    assert!(
        from <= time::OffsetDateTime::now_utc(),
        "a message signed for now must be submittable immediately, but opens at {from}"
    );

    // Deferred: the same five minutes, an hour out.
    let later_message = sign(&ctx.home_path().join("later.json"), Some("1h"));
    let (later_from, later_until) = window(&later_message);
    assert_eq!(later_until - later_from, time::Duration::minutes(5));
    let deferred = later_from - from;
    assert!(
        (deferred - time::Duration::hours(1)).abs() < time::Duration::minutes(2),
        "`--valid-from 1h` should open the window about an hour later, not {deferred}"
    );

    // An RFC 3339 timestamp names the opening directly.
    let at_message = sign(
        &ctx.home_path().join("at.json"),
        Some("2126-08-17T10:07:00Z"),
    );
    assert_eq!(at_message["summary"]["valid_from"], "2126-08-17T10:07:00Z");
    assert_eq!(at_message["summary"]["valid_until"], "2126-08-17T10:12:00Z");
}

/// A query answers immediately, so there is nothing to poll for and no
/// status check to pre-sign.
#[test]
fn signs_a_query_without_a_status_check() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("failed to write candid file");
    let out = ctx.home_path().join("query.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            UNREACHABLE,
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--query",
            "--sign-only",
            out.as_str(),
            TARGET,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    let message = read_message(&out);
    assert_eq!(message["request"]["type"], "query");
    assert!(
        message["request"].get("request_id").is_none(),
        "a query identifies nothing to poll: {message}"
    );
    assert!(
        message["request"].get("status_check").is_none(),
        "a query has no status to check: {message}"
    );
}

/// `-` writes the message to stdout, so it can be piped rather than filed.
#[test]
fn writes_to_stdout() {
    let ctx = TestContext::new();
    let did = ctx.home_path().join("service.did");
    write_string(&did, GREET_DID).expect("failed to write candid file");

    let output = ctx
        .icp()
        .args([
            "canister",
            "call",
            "--network",
            UNREACHABLE,
            "--root-key",
            "mainnet",
            "--candid",
            did.as_str(),
            "--sign-only",
            "-",
            TARGET,
            "greet",
            "(\"world\")",
        ])
        .output()
        .expect("failed to run the signing command");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let message: Value = serde_json::from_slice(&output.stdout)
        .expect("stdout must be the message and nothing else");
    assert_eq!(message["format"], "icp-signed-message");
}

/// The interface a signed message carries comes from the project's own build
/// when `--candid` is not given — the canister itself cannot be asked.
#[test]
fn embeds_the_interface_from_the_local_build() {
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

        networks:
          - name: offline-network
            mode: connected
            url: {UNREACHABLE}
            root-key: mainnet

        environments:
          - name: offline
            network: offline-network
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Build and link, both of which are local. Between them the project knows
    // the canister's id and its interface without ever reaching the network.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister", "--environment", "offline"])
        .assert()
        .success();
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "link",
            "my-canister",
            TARGET,
            "--environment",
            "offline",
        ])
        .assert()
        .success();

    let out = project_dir.join("message.json");
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "offline",
            "--sign-only",
            out.as_str(),
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success();

    let message = read_message(&out);
    assert_eq!(message["destination"]["canister"], TARGET);
    let candid = message["candid"]
        .as_str()
        .expect("the build artifact's interface should have been embedded");
    assert!(
        candid.contains("greet"),
        "embedded interface should describe the called method: {candid}"
    );
}

/// A root key that has to be fetched cannot be resolved without a network, so
/// it is refused up front rather than left to time out.
#[test]
fn refuses_to_fetch_a_root_key() {
    let ctx = TestContext::new();
    let out = ctx.home_path().join("message.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            UNREACHABLE,
            "--root-key",
            "fetch",
            "--sign-only",
            out.as_str(),
            TARGET,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .failure()
        .stderr(contains("--root-key mainnet"));
}

/// Signing a proxied call would target the proxy and return a `ProxyResult` the
/// submitting side would have to unwrap; out of scope for v1.
#[test]
fn rejects_proxy() {
    let ctx = TestContext::new();

    ctx.icp()
        .args([
            "canister",
            "call",
            "--sign-only",
            "message.json",
            "--proxy",
            "aaaaa-aa",
            TARGET,
            "greet",
        ])
        .assert()
        .failure()
        .stderr(contains("--proxy").and(contains("--sign-only")));
}

/// A duration the parser accepts can still reach past the last representable
/// instant. That has to be a CLI error, not a panic.
#[test]
fn rejects_an_unrepresentable_valid_from() {
    let ctx = TestContext::new();
    let out = ctx.home_path().join("message.json");

    ctx.icp()
        .args([
            "canister",
            "call",
            "--network",
            UNREACHABLE,
            "--root-key",
            "mainnet",
            "--sign-only",
            out.as_str(),
            "--valid-from",
            "999999999999d",
            TARGET,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .failure()
        .stderr(contains("too far from now").and(contains("--valid-from")))
        .stderr(contains("panicked").not());
}

/// `--valid-from` only means something for a message that is being signed.
#[test]
fn valid_from_requires_sign_only() {
    let ctx = TestContext::new();

    ctx.icp()
        .args([
            "canister",
            "call",
            "--valid-from",
            "1h",
            TARGET,
            "greet",
            "(\"world\")",
        ])
        .assert()
        .failure()
        .stderr(contains("--sign-only"));
}
