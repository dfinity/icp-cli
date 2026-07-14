use icp::fs::write_string;
use indoc::formatdoc;
use predicates::str::{PredicateStrExt, contains};

mod common;
use crate::common::{NETWORK_RANDOM_PORT, TestContext};

#[tokio::test]
async fn status_when_network_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    let _guard = ctx.start_network_in(&project_dir, "random-network").await;

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network"])
        .assert()
        .success()
        .stdout(contains("Url:"))
        .stdout(contains("Root Key:"))
        .stdout(contains("Candid UI Principal:"))
        .stdout(contains("Proxy Canister Principal:"));
}

#[tokio::test]
async fn status_with_json() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    let _guard = ctx.start_network_in(&project_dir, "random-network").await;

    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json_str = String::from_utf8(output).expect("output should be valid UTF-8");
    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    // Verify JSON structure
    assert!(json.get("api_url").is_some());
    assert!(json.get("root_key").is_some());
    assert!(json.get("candid_ui_principal").is_some());
    assert!(json.get("proxy_canister_principal").is_some());
}

#[tokio::test]
async fn status_fixed_port() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("icp.yaml"),
        r#"
networks:
  - name: fixed-network
    mode: managed
    gateway:
      port: 8123
"#,
    )
    .expect("failed to write project manifest");

    let _guard = ctx.start_network_in(&project_dir, "fixed-network").await;

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "fixed-network"])
        .assert()
        .success()
        .stdout(contains("Url: http://localhost:8123"));
}

#[test]
fn status_when_network_not_running() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    // Don't start the network
    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "random-network"])
        .assert()
        .failure()
        .stderr(contains(
            "unable to access network 'random-network', is it running",
        ));
}

#[test]
fn status_nonexistent_network() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    write_string(&project_dir.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "nonexistent"])
        .assert()
        .failure()
        .stderr(contains(
            "project does not contain a network named 'nonexistent'",
        ));
}

#[test]
fn status_connected_network() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest with connected network
    write_string(
        &project_dir.join("icp.yaml"),
        r#"
networks:
  - name: connected-network
    mode: connected
    url: https://ic0.app
    root-key: mainnet
"#,
    )
    .expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["network", "status", "connected-network"])
        .assert()
        .success()
        .stdout(contains("Url: https://ic0.app"));
}

/// End-to-end test of the `root-key: fetch` path with no external network:
/// start a managed local network in one project, then in a second project
/// define a connected network pointing at it with `root-key: fetch`. `icp`
/// should fetch the running network's root key trust-on-first-use, warn about
/// it, and `network status` should report the key as `fetched` and match the
/// real root key.
#[tokio::test]
async fn status_connected_network_fetches_root_key() {
    let ctx = TestContext::new();

    // Provider project: start a managed local network on a random port.
    let provider = ctx.create_project_dir("provider");
    write_string(&provider.join("icp.yaml"), NETWORK_RANDOM_PORT)
        .expect("failed to write provider manifest");
    let _guard = ctx.start_network_in(&provider, "random-network").await;
    let network = ctx.wait_for_network_descriptor(&provider, "random-network");
    ctx.ping_until_healthy(&provider, "random-network");

    // Consumer project: connect to the provider's network and fetch its root key.
    let consumer = ctx.create_project_dir("consumer");
    write_string(
        &consumer.join("icp.yaml"),
        &formatdoc! {r#"
            networks:
              - name: fetched-network
                mode: connected
                url: http://localhost:{port}
                root-key: fetch
        "#,
            port = network.gateway_port,
        },
    )
    .expect("failed to write consumer manifest");

    // Text output: key is labeled as fetched, and the CLI warns on stderr.
    ctx.icp()
        .current_dir(&consumer)
        .args(["network", "status", "fetched-network"])
        .assert()
        .success()
        .stdout(contains("Root Key:"))
        .stdout(contains("(fetched - unverified, trust-on-first-use)"))
        .stderr(contains("provenance is not verified"));

    // JSON output: root_key_source is "fetched" and the fetched key matches the
    // running network's actual root key. (The warning lands on stderr, so the
    // JSON on stdout stays clean.)
    let output = ctx
        .icp()
        .current_dir(&consumer)
        .args(["network", "status", "fetched-network", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json_str = String::from_utf8(output).expect("output should be valid UTF-8");
    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    assert_eq!(json["root_key_source"], "fetched");
    assert_eq!(
        json["root_key"]
            .as_str()
            .expect("root_key should be a string"),
        hex::encode(&network.root_key),
        "fetched root key should match the running network's root key"
    );
}

#[test]
fn status_not_in_project() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "status"])
        .assert()
        .failure()
        .stderr(contains("Error: failed to locate project directory").trim());
}

#[test]
fn status_help() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["network", "status", "--help"])
        .assert()
        .success()
        .stdout(contains("Get status information about a running network"));
}
