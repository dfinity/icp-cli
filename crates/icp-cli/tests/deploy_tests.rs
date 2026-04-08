use indoc::{formatdoc, indoc};
use predicates::{
    ord::eq,
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains},
};
use test_tag::tag;

use crate::common::{
    ENVIRONMENT_DOCKER_ENGINE, ENVIRONMENT_RANDOM_PORT, NETWORK_DOCKER_ENGINE, NETWORK_RANDOM_PORT,
    TestContext, clients,
};
use icp::{
    fs::{create_dir_all, write_string},
    prelude::*,
};

mod common;

#[test]
fn deploy_empty() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = indoc! {r#"
        canisters:
            - canisters/*
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--subnet", common::SUBNET_ID])
        .assert()
        .success();
}

#[test]
fn deploy_canister_not_found() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Project manifest
    let pm = indoc! {r#"
        canisters:
            - canisters/*
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        pm,                            // contents
    )
    .expect("failed to write project manifest");

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "my-canister", "--subnet", common::SUBNET_ID])
        .assert()
        .failure()
        .stderr(contains("Error: project does not contain a canister named 'my-canister'").trim());
}

#[tokio::test]
async fn deploy() {
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
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy project
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[tokio::test]
async fn deploy_twice_should_succeed() {
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
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy project (first time)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Deploy project (second time)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Query canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "my-canister",
            "greet",
            "(\"test\")",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[tokio::test]
async fn canister_create_colocates_canisters() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = formatdoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"
          - name: canister-b
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-c
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-d
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-e
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"

          - name: canister-f
            build:
              steps:
                - type: script
                  command: touch "$ICP_WASM_OUTPUT_PATH"
        networks:
          - name: random-network
            mode: managed
            gateway:
                port: 0
            subnets: [application, application, application]
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy first three canisters
    let icp_client = clients::icp(&ctx, &project_dir, Some("random-environment".to_string()));
    icp_client.mint_cycles(20 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "canister-a",
            "canister-b",
            "canister-c",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure(); // no valid wasm - should fail but still creates canisters

    let registry = clients::registry(&ctx);

    let subnet_a = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-a"))
        .await;

    let subnet_b = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-b"))
        .await;

    let subnet_c = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-c"))
        .await;

    assert_eq!(
        subnet_a, subnet_b,
        "Canister A and B should be on the same subnet"
    );
    assert_eq!(
        subnet_a, subnet_c,
        "Canister B and C should be on the same subnet"
    );

    // Deploy remaining canisters
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "canister-d",
            "canister-e",
            "canister-f",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure(); // no valid wasm - should fail but still creates canisters

    let subnet_d = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-d"))
        .await;

    let subnet_e = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-e"))
        .await;

    let subnet_f = registry
        .get_subnet_for_canister(icp_client.get_canister_id("canister-f"))
        .await;

    assert_eq!(
        subnet_a, subnet_d,
        "Canister D should be on the same subnet as canister A"
    );
    assert_eq!(
        subnet_a, subnet_e,
        "Canister E should be on the same subnet as canister A"
    );
    assert_eq!(
        subnet_a, subnet_f,
        "Canister F should be on the same subnet as canister A"
    );
}

#[tokio::test]
async fn deploy_prints_canister_urls() {
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
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy project and verify Candid UI URLs are printed
    // The example_icp_mo.wasm doesn't have http_request, so it should show Candid UI
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Deployed canisters:"))
        .stdout(contains("my-canister (Candid UI):"))
        .stdout(contains(".localhost:"))
        .stdout(contains("?id="));
}

#[tokio::test]
async fn deploy_prints_friendly_url_for_asset_canister() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");
    let assets_dir = project_dir.join("www");
    create_dir_all(&assets_dir).expect("failed to create assets directory");
    write_string(&assets_dir.join("index.html"), "hello").expect("failed to create index page");

    // Project manifest with a pre-built asset canister
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
                    - {assets_dir}

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy and check that the friendly URL is printed (not the Candid UI form)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("Deployed canisters:"))
        .stdout(contains(
            "my-canister: http://my-canister.random-environment.localhost:",
        ));
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_upgrade_rejects_incompatible_candid() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Write mops.toml for the Motoko toolchain
    write_string(
        &project_dir.join("mops.toml"),
        indoc! {r#"
            [dependencies]
            base = "0.16.0"

            [toolchain]
            moc = "0.16.3"
        "#},
    )
    .expect("failed to write mops.toml");

    // Initial version: greet takes Text
    write_string(
        &project_dir.join("main.mo"),
        indoc! {"
            persistent actor {
                public query func greet(name : Text) : async Text {
                    return \"Hello, \" # name # \"!\";
                };
            };
        "},
    )
    .expect("failed to write main.mo");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network and deploy initial version
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

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

    // Verify initial version works
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Breaking change: greet now takes Nat instead of Text
    write_string(
        &project_dir.join("main.mo"),
        indoc! {"
            import Nat \"mo:base/Nat\";

            persistent actor {
                public query func greet(n : Nat) : async Text {
                    return \"Hello, \" # Nat.toText(n) # \"!\";
                };
            };
        "},
    )
    .expect("failed to write updated main.mo");

    // Deploy upgrade should fail with candid incompatibility error
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains("Candid interface compatibility check failed"));

    // Deploy with --yes should succeed
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--yes",
        ])
        .assert()
        .success();

    // Verify updated version works
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(42)",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, 42!\")").trim());
}

#[tag(docker)]
#[tokio::test]
async fn deploy_cloud_engine() {
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

        {NETWORK_DOCKER_ENGINE}
        {ENVIRONMENT_DOCKER_ENGINE}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.docker_pull_engine_network();
    let _guard = ctx
        .start_network_in(&project_dir, "docker-engine-network")
        .await;
    ctx.ping_until_healthy(&project_dir, "docker-engine-network");

    // Find the CloudEngine subnet by querying the topology endpoint
    // TODO replace with a subnet selection parameter once we have one
    let topology_url = ctx.gateway_url().join("/_/topology").unwrap();
    let topology: serde_json::Value = reqwest::get(topology_url)
        .await
        .expect("failed to fetch topology")
        .json()
        .await
        .expect("failed to parse topology");

    let subnet_configs = topology["subnet_configs"]
        .as_object()
        .expect("subnet_configs should be an object");
    let cloud_engine_subnet_id = subnet_configs
        .iter()
        .find_map(|(id, config)| {
            (config["subnet_kind"].as_str()? == "CloudEngine").then_some(id.clone())
        })
        .expect("no CloudEngine subnet found in topology");

    // Deploy to the CloudEngine subnet
    // Only the admin can do this. In local envs, the admin is the anonymous principal
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "--subnet",
            &cloud_engine_subnet_id,
            "--environment",
            "docker-engine-environment",
        ])
        .assert()
        .success();

    // Query canister to verify it works
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "docker-engine-environment",
            "my-canister",
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_inline_args_candid() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

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
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args",
            "(opt (42 : nat8))",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"42\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_args_file() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");
    write_string(&project_dir.join("args.txt"), "(opt (42 : nat8))")
        .expect("failed to write args file");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args-file",
            "args.txt",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"42\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn deploy_with_args_hex_format() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Hex encoding of "(opt 100 : opt nat8)" — didc encode '(opt 100 : opt nat8)'
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "my-canister",
            "--environment",
            "random-environment",
            "--args",
            "4449444c016e7b01000164",
            "--args-format",
            "hex",
        ])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "get",
            "()",
        ])
        .assert()
        .success()
        .stdout(eq("(\"100\")").trim());
}

#[test]
fn deploy_with_args_multiple_canisters_fails() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let pm = indoc! {r#"
        canisters:
          - name: canister-a
            build:
              steps:
                - type: script
                  command: echo hi
          - name: canister-b
            build:
              steps:
                - type: script
                  command: echo hi
    "#};

    write_string(&project_dir.join("icp.yaml"), pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "deploy",
            "canister-a",
            "canister-b",
            "--subnet",
            common::SUBNET_ID,
            "--args",
            "()",
        ])
        .assert()
        .failure()
        .stderr(contains(
            "--args and --args-file can only be used when deploying a single canister",
        ));
}

#[tokio::test]
async fn deploy_through_proxy() {
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

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    let proxy_cid = ctx.get_proxy_cid(&project_dir, "random-network");

    // Deploy through proxy
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

    // Verify canister works by calling it through proxy (proxy is the controller)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "my-canister",
            "greet",
            "(\"proxy\")",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, proxy!\")").trim());

    // Verify canister status through proxy shows the proxy as controller
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "my-canister",
            "--environment",
            "random-environment",
            "--proxy",
            &proxy_cid,
        ])
        .assert()
        .success()
        .stdout(contains("Status: Running").and(contains(&proxy_cid)));
}
