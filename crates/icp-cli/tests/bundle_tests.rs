use std::{
    fs,
    io::{BufReader, Read as _},
};

use flate2::bufread::GzDecoder;
use icp::{
    fs::{create_dir_all, json, write_string},
    prelude::*,
    store_id::IdMapping,
};
use indoc::formatdoc;
use predicates::{
    ord::eq,
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains},
};
use tar::Archive;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};

mod common;

/// Bundle a standard frontend-backend project: a script-built backend canister and an
/// asset-canister-recipe frontend canister with an assets sync step.
/// Verify archive structure, manifest content, and that the bundle deploys successfully.
#[tokio::test]
async fn bundle_and_deploy() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    // A small WASM to serve as the pre-built artifact for the backend.
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    // Create asset directory for the frontend canister.
    let asset_dir = project_dir.join("www");
    create_dir_all(&asset_dir).expect("failed to create asset dir");
    write_string(&asset_dir.join("index.html"), "hello").expect("failed to write asset file");

    let pm = formatdoc! {r#"
        canisters:
          - name: backend
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"

          - name: frontend
            recipe:
              type: "@dfinity/asset-canister@v2.1.0"
              configuration:
                dir: www

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let bundle_path = project_dir.join("bundle.tar.gz");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", bundle_path.as_str()])
        .assert()
        .success();

    assert!(bundle_path.exists(), "bundle file was not created");

    // Extract and inspect archive contents.
    let bundle_bytes = fs::read(bundle_path.as_std_path()).expect("failed to read bundle");
    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);

    let mut found_manifest = false;
    let mut found_backend_wasm = false;
    let mut found_frontend_wasm = false;
    let mut found_asset = false;
    let mut manifest_yaml = String::new();

    for entry in archive.entries().expect("failed to read archive entries") {
        let mut entry = entry.expect("failed to read archive entry");
        let path = entry
            .path()
            .expect("failed to get entry path")
            .to_string_lossy()
            .into_owned();

        match path.as_str() {
            "icp.yaml" => {
                found_manifest = true;
                entry
                    .read_to_string(&mut manifest_yaml)
                    .expect("failed to read icp.yaml");
            }
            "backend.wasm" => {
                found_backend_wasm = true;
            }
            "frontend.wasm" => {
                found_frontend_wasm = true;
            }
            p if p.starts_with("frontend/www/") => {
                found_asset = true;
            }
            _ => {}
        }
    }

    assert!(found_manifest, "icp.yaml not found in bundle");
    assert!(found_backend_wasm, "backend.wasm not found in bundle");
    assert!(found_frontend_wasm, "frontend.wasm not found in bundle");
    assert!(
        found_asset,
        "asset file not found under frontend/www/ in bundle"
    );

    // Manifest must contain pre-built steps and no script or recipe steps.
    assert!(
        manifest_yaml.contains("pre-built"),
        "bundle manifest should have pre-built build steps"
    );
    assert!(
        !manifest_yaml.contains("type: script"),
        "bundle manifest should not contain script steps"
    );
    assert!(
        !manifest_yaml.contains("recipe:"),
        "bundle manifest should not contain recipe sections"
    );
    assert!(
        manifest_yaml.contains("sha256:"),
        "bundle manifest should include sha256 for pre-built wasms"
    );

    // Extract bundle to a fresh directory and deploy from it.
    let bundle_dir = project_dir.join("bundle-extracted");
    create_dir_all(&bundle_dir).expect("failed to create bundle-extracted dir");

    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);
    archive
        .unpack(bundle_dir.as_std_path())
        .expect("failed to extract bundle");

    let _g = ctx.start_network_in(&bundle_dir, "random-network").await;
    ctx.ping_until_healthy(&bundle_dir, "random-network");

    let network_port = ctx
        .wait_for_network_descriptor(&bundle_dir, "random-network")
        .gateway_port;

    clients::icp(&ctx, &bundle_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&bundle_dir)
        .args([
            "deploy",
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify the backend canister responds to a query call.
    ctx.icp()
        .current_dir(&bundle_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "backend",
            "greet",
            "(\"world\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, world!\")").trim());

    // Verify the frontend canister serves the bundled asset.
    let id_mapping: IdMapping = json::load(
        &bundle_dir
            .join(".icp")
            .join("cache")
            .join("mappings")
            .join("random-environment.ids.json"),
    )
    .expect("failed to read ID mapping");

    let frontend_cid = id_mapping
        .get("frontend")
        .expect("frontend canister ID not found");

    let resp = reqwest::get(format!(
        "http://localhost:{network_port}/?canisterId={frontend_cid}"
    ))
    .await
    .expect("request to frontend canister failed");

    assert_eq!(
        resp.text().await.expect("failed to read response body"),
        "hello"
    );
}

/// Projects with script sync steps must be rejected with a clear error.
#[test]
fn bundle_rejects_script_sync_step() {
    let ctx = TestContext::new();

    let project_dir = ctx.create_project_dir("icp");

    let pm = r#"
canisters:
  - name: my-canister
    build:
      steps:
        - type: script
          command: echo build
    sync:
      steps:
        - type: script
          command: echo sync
"#;

    write_string(&project_dir.join("icp.yaml"), pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("my-canister").and(contains("script sync step")));
}
