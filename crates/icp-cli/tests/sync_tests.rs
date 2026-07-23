use icp::{
    fs::{create_dir_all, write_string},
    prelude::*,
    store_id::IdMapping,
};
use indoc::formatdoc;
use predicates::{
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains},
};

use crate::common::{
    ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, build_sync_plugin_example, clients,
};

mod common;

#[tokio::test]
async fn sync_adapter_script_single() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing"

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

    // Deploy project (it should sync as well)
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["--debug", "deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stderr(contains("syncing").trim());

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args([
            "--debug",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stderr(contains("syncing").trim());
}

#[tokio::test]
async fn sync_adapter_script_multiple() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "second"
                - type: script
                  command: echo "first"

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

    // Deploy project (it should sync as well)
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["--debug", "deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stderr(contains("first").and(contains("second")));

    // Invoke sync
    ctx.icp()
        .current_dir(project_dir)
        .args([
            "--debug",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stderr(contains("first").and(contains("second")));
}

/// `icp sync` does not manage canister lifecycle (unlike `icp deploy`, it must
/// not auto-start a canister the user may have stopped deliberately). When the
/// target canister is not Running it must abort up front with an actionable
/// error instead of letting the sync plugin's first call fail with a cryptic
/// IC0508. The sync step must never run.
#[tokio::test]
async fn sync_aborts_when_canister_not_running() {
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
            sync:
              steps:
                - type: script
                  command: echo "syncing"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy leaves the canister Running.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Stop it: sync must detect this and abort rather than auto-start.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "stop",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // sync aborts early with an actionable message; the `echo "syncing"` step
    // never runs, so its runtime progress output must not appear. (The `--debug`
    // config dump echoes the step's command text, so we check for the runtime
    // `DEBUG icp::progress: syncing` marker rather than the bare word "syncing".)
    ctx.icp()
        .current_dir(&project_dir)
        .env("NO_COLOR", "1")
        .args([
            "--debug",
            "sync",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(
            contains("asset sync requires it to be Running")
                .and(contains("icp canister start"))
                .and(contains("DEBUG icp::progress: syncing").not()),
        );
}

/// The `assets` sync step type was removed. A manifest that still uses it must
/// fail to load with a helpful, targeted message (and must not name a specific
/// recipe).
#[tokio::test]
async fn sync_step_assets_is_rejected() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: pre-built
                  url: https://github.com/dfinity/sdk/raw/refs/tags/0.27.0/src/distributed/assetstorage.wasm.gz
                  sha256: 865eb25df5a6d857147e078bb33c727797957247f7af2635846d65c5397b36a6

            sync:
              steps:
                - type: assets
                  dirs:
                    - www
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "show"])
        .assert()
        .failure()
        .stderr(
            contains("no longer supports")
                .and(contains("assets"))
                // The message stays recipe-agnostic; it must not leak a specific
                // recipe identifier.
                .and(contains("@dfinity/asset-canister").not()),
        );
}

#[tokio::test]
async fn sync_with_valid_principal() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
            sync:
              steps:
                - type: script
                  command: echo syncing
        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Valid principal
    let principal = "aaaaa-aa";

    // Try to sync with principal (should fail)
    ctx.icp()
        .current_dir(&project_dir)
        .args(["sync", principal, "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(contains("project does not contain a canister named"));
}

#[tokio::test]
async fn sync_multiple_canisters() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with multiple canisters
    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-a"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-b"
          - name: canister-c
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-c"

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

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Sync multiple canisters
    ctx.icp()
        .current_dir(project_dir)
        .env("NO_COLOR", "1")
        .args([
            "--debug",
            "sync",
            "canister-a",
            "canister-b",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stderr(contains("Syncing canisters"))
        .stderr(contains(r#"canisters: ["canister-a", "canister-b"]"#))
        .stderr(contains("DEBUG icp::progress: syncing canister-a"))
        .stderr(contains("DEBUG icp::progress: syncing canister-b"))
        .stderr(contains("DEBUG icp::progress: syncing canister-c").not());
}

#[tokio::test]
async fn sync_plugin_registers_seed_data() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // Create seed-data directory with fruit files
    let seed_data = project_dir.join("seed-data");
    create_dir_all(&seed_data).expect("failed to create seed-data");
    write_string(&seed_data.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");
    write_string(&seed_data.join("fruit-02.txt"), "banana").expect("failed to write fruit-02.txt");
    write_string(&seed_data.join("fruit-03.txt"), "cherry").expect("failed to write fruit-03.txt");

    // Manifest: pre-built canister wasm + plugin sync step pointing at the pre-built plugin wasm.
    // dirs is relative to the project directory and preopened read-only inside the plugin's WASI sandbox.
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Mint cycles and deploy. deploy also runs the sync step: the plugin calls
    // set_uploader (user is controller, so the direct call is permitted), then
    // calls register for each fruit file directly with the user identity as the uploader.
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Query the canister to verify all three fruits were registered
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "show",
            "()",
            "--query",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("apple")
                .and(contains("banana"))
                .and(contains("cherry")),
        );
}

/// A malformed `ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS` must abort the sync with an
/// actionable error rather than being silently ignored. This also exercises the
/// end-to-end wiring: it proves the override is actually read on the real plugin
/// sync path (if it weren't, the bogus value would be ignored and the sync would
/// proceed), which the unit tests can't cover on their own.
#[tokio::test]
async fn sync_plugin_rejects_invalid_compute_limit_env() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    let seed_data = project_dir.join("seed-data");
    create_dir_all(&seed_data).expect("failed to create seed-data");
    write_string(&seed_data.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // deploy runs the sync step, which reads ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS.
    // A non-integer value must abort with a message that names the variable and
    // echoes the offending value.
    ctx.icp()
        .current_dir(&project_dir)
        .env("NO_COLOR", "1")
        .env("ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS", "not-a-number")
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(
            contains("invalid ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS value")
                .and(contains("not-a-number")),
        );
}

/// A `dirs:` entry that is a symlink (here pointing outside the project) is
/// rejected before the plugin runs, so a preopen cannot escape the canister
/// directory. Symlinks are forbidden outright for now — see
/// `crates/icp-sync-plugin/DESIGN.md`.
#[cfg(unix)]
#[tokio::test]
async fn sync_plugin_rejects_symlinked_dir() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // A real directory *outside* the project, and a symlink to it inside the
    // project that the manifest declares as a `dirs:` entry.
    let outside = ctx.home_path().join("outside-seed-data");
    create_dir_all(&outside).expect("failed to create outside dir");
    write_string(&outside.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");
    std::os::unix::fs::symlink(&outside, project_dir.join("seed-data"))
        .expect("failed to create symlink");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(contains("symlink").and(contains("seed-data")));
}

/// A `files:` entry that is a symlink (here pointing outside the project) is
/// rejected before the host reads it, so a read cannot escape the canister
/// directory.
#[cfg(unix)]
#[tokio::test]
async fn sync_plugin_rejects_symlinked_file() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // A real file *outside* the project, and a symlink to it inside the project
    // that the manifest declares as a `files:` entry.
    let outside = ctx.home_path().join("outside-secret.txt");
    write_string(&outside, "secret").expect("failed to write outside file");
    std::os::unix::fs::symlink(&outside, project_dir.join("config.txt"))
        .expect("failed to create symlink");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  files:
                    - config.txt

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(contains("symlink").and(contains("config.txt")));
}

#[tokio::test]
async fn sync_script_icp_env_vars() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // canister-a verifies all four env vars; canister-b verifies cross-canister CID visibility.
    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "ENV=$ICP_CLI_ENVIRONMENT NET=$ICP_CLI_NETWORK CID=$ICP_CLI_CID B_CID=$ICP_CLI_CID_CANISTER_B"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "B_SEES_A=$ICP_CLI_CID_CANISTER_A"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    let id_mapping: IdMapping = icp::fs::json::load(
        &project_dir
            .join(".icp")
            .join("cache")
            .join("mappings")
            .join("random-environment.ids.json"),
    )
    .expect("failed to read ID mapping");

    let cid_a = id_mapping
        .get("canister-a")
        .expect("canister-a ID not found")
        .to_text();

    let cid_b = id_mapping
        .get("canister-b")
        .expect("canister-b ID not found")
        .to_text();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["--debug", "sync", "--environment", "random-environment"])
        .assert()
        .success()
        .stderr(contains("ENV=random-environment"))
        .stderr(contains("NET=random-network"))
        .stderr(contains(format!("CID={cid_a}")))
        .stderr(contains(format!("B_CID={cid_b}")))
        .stderr(contains(format!("B_SEES_A={cid_a}")));
}

#[tokio::test]
async fn sync_plugin_routes_through_proxy() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let (canister_wasm, plugin_wasm) = build_sync_plugin_example();

    // Create seed-data directory with fruit files
    let seed_data = project_dir.join("seed-data");
    create_dir_all(&seed_data).expect("failed to create seed-data");
    write_string(&seed_data.join("fruit-01.txt"), "apple").expect("failed to write fruit-01.txt");
    write_string(&seed_data.join("fruit-02.txt"), "banana").expect("failed to write fruit-02.txt");
    write_string(&seed_data.join("fruit-03.txt"), "cherry").expect("failed to write fruit-03.txt");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: {plugin_wasm}
                  dirs:
                    - seed-data

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network (the proxy canister is automatically deployed)
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy through proxy so the proxy canister becomes a controller of my-canister.
    // deploy also runs the sync step: the plugin routes set_uploader through the proxy
    // (direct: false, proxy is controller), then calls register directly with the user identity.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--proxy",
            &proxy_cid,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query the canister to verify all three fruits were registered
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "show",
            "()",
            "--query",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("apple")
                .and(contains("banana"))
                .and(contains("cherry")),
        );
}

#[tokio::test]
async fn sync_all_canisters_in_environment() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with multiple canisters and environments
    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-a"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-b"
          - name: canister-c
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: script
                  command: echo "syncing canister-c"

        {NETWORK_RANDOM_PORT}
        
        environments:
          - name: test-env
            network: random-network
            canisters:
              - canister-a
              - canister-b
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

    // Deploy project
    clients::icp(&ctx, &project_dir, Some("test-env".to_string())).mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "test-env"])
        .assert()
        .success();

    // Sync all canisters in environment (no canister names specified)
    ctx.icp()
        .current_dir(project_dir)
        .env("NO_COLOR", "1")
        .args(["--debug", "sync", "--environment", "test-env"])
        .assert()
        .success()
        .stderr(contains("Syncing canisters"))
        .stderr(contains(r#"canisters: []"#))
        .stderr(contains(r#"environment: Some("test-env")"#))
        .stderr(contains("DEBUG icp::progress: syncing canister-a"))
        .stderr(contains("DEBUG icp::progress: syncing canister-b"))
        .stderr(contains("DEBUG icp::progress: syncing canister-c").not()); // not in test-env
}
