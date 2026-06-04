use std::{
    fs,
    io::{BufReader, Read as _},
};

use camino::Utf8Component;
use flate2::bufread::GzDecoder;
use icp::{
    fs::{create_dir_all, json, write, write_string},
    prelude::*,
    store_id::IdMapping,
};
use indoc::formatdoc;
use predicates::{
    ord::eq,
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains},
};
use sha2::{Digest, Sha256};
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
            "canisters/backend.wasm" => {
                found_backend_wasm = true;
            }
            "canisters/frontend.wasm" => {
                found_frontend_wasm = true;
            }
            p if p.starts_with("canisters/frontend/www/") => {
                found_asset = true;
            }
            _ => {}
        }
    }

    assert!(found_manifest, "icp.yaml not found in bundle");
    assert!(
        found_backend_wasm,
        "canisters/backend.wasm not found in bundle"
    );
    assert!(
        found_frontend_wasm,
        "canisters/frontend.wasm not found in bundle"
    );
    assert!(
        found_asset,
        "asset file not found under canisters/frontend/www/ in bundle"
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
        .args(["deploy", "--environment", "random-environment"])
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

/// Bundle a canister whose environment override uses an external init_args file.
/// The file must be copied into the archive at a normalized path and the manifest
/// reference rewritten to match.
#[test]
fn bundle_inlines_external_init_args_file() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    write_string(&project_dir.join("args.idl"), "(\"world\")").expect("failed to write args file");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"

        networks:
          - name: random-network
            mode: managed
            gateway:
              port: 0

        environments:
          - name: random-environment
            network: random-network
            init_args:
              my-canister:
                path: ./args.idl
                format: candid
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let bundle_path = project_dir.join("bundle.tar.gz");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", bundle_path.as_str()])
        .assert()
        .success();

    let bundle_bytes = fs::read(bundle_path.as_std_path()).expect("failed to read bundle");
    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);

    let mut found_args_file = false;
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
                entry
                    .read_to_string(&mut manifest_yaml)
                    .expect("failed to read icp.yaml");
            }
            "init-args/my-canister/args.idl" => {
                found_args_file = true;
            }
            _ => {}
        }
    }

    assert!(
        found_args_file,
        "init-args/my-canister/args.idl not found in bundle"
    );
    assert!(
        manifest_yaml.contains("init-args/my-canister/args.idl"),
        "bundle manifest should reference the relocated init_args file"
    );
    assert!(
        !manifest_yaml.contains("./args.idl"),
        "bundle manifest should not contain the original relative path"
    );
}

/// Canister names with characters invalid in file paths (spaces, `!`, `/`, etc.)
/// must be sanitized for archive entry names while the manifest preserves the
/// original name.
#[test]
fn bundle_sanitizes_canister_name_for_paths() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: "my canister!"
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let bundle_path = project_dir.join("bundle.tar.gz");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", bundle_path.as_str()])
        .assert()
        .success();

    let bundle_bytes = fs::read(bundle_path.as_std_path()).expect("failed to read bundle");
    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);

    let mut found_wasm = false;
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
                entry
                    .read_to_string(&mut manifest_yaml)
                    .expect("failed to read icp.yaml");
            }
            "canisters/my_canister_.wasm" => {
                found_wasm = true;
            }
            _ => {}
        }
    }

    assert!(
        found_wasm,
        "canisters/my_canister_.wasm not found in bundle"
    );
    assert!(
        manifest_yaml.contains("my_canister_.wasm"),
        "bundle manifest should reference sanitized wasm filename"
    );
    assert!(
        manifest_yaml.contains("my canister!"),
        "bundle manifest should preserve original canister name"
    );
}

/// An asset sync `dir` that contains `..` components but still resolves inside the project
/// must be accepted, and the `..` components must be lexically resolved before the path is
/// written into the archive or the rewritten manifest.
#[test]
fn bundle_normalizes_dotdot_within_project() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    // Assets live inside the project at ./shared-assets. The canister references them via
    // a path that goes `tmp/..` and resolves to the same location — proof that `..` is
    // handled lexically and doesn't have to point outside.
    let assets_dir = project_dir.join("shared-assets");
    create_dir_all(&assets_dir).expect("failed to create shared-assets dir");
    write_string(&assets_dir.join("index.html"), "hello").expect("failed to write asset");
    create_dir_all(&project_dir.join("tmp")).expect("failed to create tmp dir");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: assets
                  dir: tmp/../shared-assets
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let bundle_path = project_dir.join("bundle.tar.gz");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", bundle_path.as_str()])
        .assert()
        .success();

    let bundle_bytes = fs::read(bundle_path.as_std_path()).expect("failed to read bundle");
    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);

    let mut found_asset = false;
    let mut manifest_yaml = String::new();

    for entry in archive.entries().expect("failed to read archive entries") {
        let mut entry = entry.expect("failed to read archive entry");
        let path = entry
            .path()
            .expect("failed to get entry path")
            .to_string_lossy()
            .into_owned();

        // Reject only `..` *path components* — substring matching would false-positive on
        // a legitimate filename like `foo..bar.txt`.
        if Path::new(&path)
            .components()
            .any(|c| matches!(c, Utf8Component::ParentDir))
        {
            panic!("archive entry '{path}' contains a '..' path component");
        }

        match path.as_str() {
            "icp.yaml" => {
                entry
                    .read_to_string(&mut manifest_yaml)
                    .expect("failed to read icp.yaml");
            }
            p if p.starts_with("canisters/my-canister/shared-assets/") => {
                found_asset = true;
            }
            _ => {}
        }
    }

    assert!(
        found_asset,
        "asset not found under canisters/my-canister/shared-assets/ in bundle"
    );

    // Parse the rewritten manifest and inspect the actual sync dir field rather than
    // substring-matching the whole YAML.
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&manifest_yaml).expect("manifest yaml is invalid");
    let dir = parsed["canisters"][0]["sync"]["steps"][0]["dir"]
        .as_str()
        .expect("expected sync step 0 to have a string `dir` field");
    assert_eq!(dir, "canisters/my-canister/shared-assets");
    assert!(
        !Path::new(dir)
            .components()
            .any(|c| matches!(c, Utf8Component::ParentDir)),
        "bundle manifest sync dir should not contain a '..' component: {dir}"
    );
}

/// An asset sync step whose `dir` resolves *outside* the project directory must be rejected.
/// Bundles can only reference files inside the project so the produced archive is portable.
#[test]
fn bundle_rejects_source_outside_project() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    // Assets live one directory above the project — outside its tree.
    let assets_dir = project_dir
        .parent()
        .expect("project dir has no parent")
        .join("shared-assets");
    create_dir_all(&assets_dir).expect("failed to create sibling assets dir");
    write_string(&assets_dir.join("index.html"), "hello").expect("failed to write asset");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: assets
                  dir: ../shared-assets
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("my-canister").and(contains("outside the project directory")));
}

/// Bundle a canister with two plugin sync steps and verify the archive layout: per-plugin
/// wasm at `plugins/{canister}/{idx}.wasm`, preopened dirs at `plugins/{canister}/{idx}/dirs/`,
/// input files at `plugins/{canister}/{idx}/files/`. Also verify the rewritten manifest
/// references those paths and includes a sha256 matching the bundled plugin bytes.
#[test]
fn bundle_packages_plugin_sync_steps() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    // Plugin bundling reads bytes and packages them — the wasm content does not need to be
    // executable here, so any non-empty byte sequence works.
    let plugin_a_bytes: &[u8] = b"\x00asm\x01\x00\x00\x00plugin-a";
    let plugin_b_bytes: &[u8] = b"\x00asm\x01\x00\x00\x00plugin-b";
    write(&project_dir.join("plugin-a.wasm"), plugin_a_bytes).expect("failed to write plugin-a");
    write(&project_dir.join("plugin-b.wasm"), plugin_b_bytes).expect("failed to write plugin-b");

    let dir_a = project_dir.join("data-a");
    create_dir_all(&dir_a).expect("failed to create data-a");
    write_string(&dir_a.join("a.txt"), "alpha").expect("failed to write a.txt");

    let dir_b = project_dir.join("data-b");
    create_dir_all(&dir_b).expect("failed to create data-b");
    write_string(&dir_b.join("b.txt"), "bravo").expect("failed to write b.txt");

    write_string(&project_dir.join("config.toml"), "key=value").expect("failed to write config");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: plugin
                  path: plugin-a.wasm
                  dirs:
                    - data-a
                - type: plugin
                  path: plugin-b.wasm
                  dirs:
                    - data-b
                  files:
                    - config.toml
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let bundle_path = project_dir.join("bundle.tar.gz");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", bundle_path.as_str()])
        .assert()
        .success();

    let bundle_bytes = fs::read(bundle_path.as_std_path()).expect("failed to read bundle");
    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);

    let mut found_plugin_a: Option<Vec<u8>> = None;
    let mut found_plugin_b: Option<Vec<u8>> = None;
    let mut found_a_dir = false;
    let mut found_b_dir = false;
    let mut found_config = false;
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
                entry
                    .read_to_string(&mut manifest_yaml)
                    .expect("failed to read icp.yaml");
            }
            "plugins/my-canister/0.wasm" => {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .expect("failed to read plugin-a wasm");
                found_plugin_a = Some(buf);
            }
            "plugins/my-canister/1.wasm" => {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .expect("failed to read plugin-b wasm");
                found_plugin_b = Some(buf);
            }
            "plugins/my-canister/0/dirs/data-a/a.txt" => found_a_dir = true,
            "plugins/my-canister/1/dirs/data-b/b.txt" => found_b_dir = true,
            "plugins/my-canister/1/files/config.toml" => found_config = true,
            _ => {}
        }
    }

    assert_eq!(
        found_plugin_a.as_deref(),
        Some(plugin_a_bytes),
        "plugin-a wasm bytes in archive don't match source"
    );
    assert_eq!(
        found_plugin_b.as_deref(),
        Some(plugin_b_bytes),
        "plugin-b wasm bytes in archive don't match source"
    );
    assert!(
        found_a_dir,
        "plugin-a preopened dir not at plugins/my-canister/0/dirs/data-a/"
    );
    assert!(
        found_b_dir,
        "plugin-b preopened dir not at plugins/my-canister/1/dirs/data-b/"
    );
    assert!(
        found_config,
        "plugin-b input file not at plugins/my-canister/1/files/config.toml"
    );

    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&manifest_yaml).expect("manifest yaml is invalid");
    let steps = &parsed["canisters"][0]["sync"]["steps"];
    let plugin_a_sha = hex::encode(Sha256::digest(plugin_a_bytes));
    let plugin_b_sha = hex::encode(Sha256::digest(plugin_b_bytes));

    assert_eq!(
        steps[0]["path"].as_str(),
        Some("plugins/my-canister/0.wasm")
    );
    assert_eq!(steps[0]["sha256"].as_str(), Some(plugin_a_sha.as_str()));
    assert_eq!(
        steps[0]["dirs"][0].as_str(),
        Some("plugins/my-canister/0/dirs/data-a")
    );

    assert_eq!(
        steps[1]["path"].as_str(),
        Some("plugins/my-canister/1.wasm")
    );
    assert_eq!(steps[1]["sha256"].as_str(), Some(plugin_b_sha.as_str()));
    assert_eq!(
        steps[1]["dirs"][0].as_str(),
        Some("plugins/my-canister/1/dirs/data-b")
    );
    assert_eq!(
        steps[1]["files"][0].as_str(),
        Some("plugins/my-canister/1/files/config.toml")
    );
}

/// An `icp_manifest.yaml` next to the project manifest must be included in the bundle, with its
/// top-level `screenshots` paths relocated under a top-level `screenshots/` folder and the
/// referenced image files copied alongside. Unrelated metadata is preserved.
#[test]
fn bundle_includes_app_manifest_screenshots() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    let shots_dir = project_dir.join("media");
    create_dir_all(&shots_dir).expect("failed to create media dir");
    write(&shots_dir.join("home.png"), b"home-bytes").expect("failed to write home.png");
    write(&shots_dir.join("detail.png"), b"detail-bytes").expect("failed to write detail.png");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let app_manifest = formatdoc! {r#"
        name: My App
        description: an app we do not parse
        screenshots:
          - media/home.png
          - media/detail.png
    "#};
    write_string(&project_dir.join("icp_manifest.yaml"), &app_manifest)
        .expect("failed to write app manifest");

    let bundle_path = project_dir.join("bundle.tar.gz");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", bundle_path.as_str()])
        .assert()
        .success();

    let bundle_bytes = fs::read(bundle_path.as_std_path()).expect("failed to read bundle");
    let gz = GzDecoder::new(BufReader::new(bundle_bytes.as_slice()));
    let mut archive = Archive::new(gz);

    let mut home_bytes: Option<Vec<u8>> = None;
    let mut detail_bytes: Option<Vec<u8>> = None;
    let mut app_manifest_yaml = String::new();

    for entry in archive.entries().expect("failed to read archive entries") {
        let mut entry = entry.expect("failed to read archive entry");
        let path = entry
            .path()
            .expect("failed to get entry path")
            .to_string_lossy()
            .into_owned();

        match path.as_str() {
            "icp_manifest.yaml" => {
                entry
                    .read_to_string(&mut app_manifest_yaml)
                    .expect("failed to read icp_manifest.yaml");
            }
            "screenshots/home.png" => {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .expect("failed to read home.png");
                home_bytes = Some(buf);
            }
            "screenshots/detail.png" => {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .expect("failed to read detail.png");
                detail_bytes = Some(buf);
            }
            _ => {}
        }
    }

    assert_eq!(
        home_bytes.as_deref(),
        Some(b"home-bytes".as_slice()),
        "screenshots/home.png missing or wrong content"
    );
    assert_eq!(
        detail_bytes.as_deref(),
        Some(b"detail-bytes".as_slice()),
        "screenshots/detail.png missing or wrong content"
    );

    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&app_manifest_yaml).expect("app manifest yaml is invalid");
    assert_eq!(
        parsed["screenshots"][0].as_str(),
        Some("screenshots/home.png")
    );
    assert_eq!(
        parsed["screenshots"][1].as_str(),
        Some("screenshots/detail.png")
    );
    // Unrelated metadata must survive the rewrite.
    assert_eq!(parsed["name"].as_str(), Some("My App"));
    assert_eq!(
        parsed["description"].as_str(),
        Some("an app we do not parse")
    );
}

/// Two screenshots whose basenames collide under the flat `screenshots/` folder must be rejected.
#[test]
fn bundle_rejects_screenshot_name_collision() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    create_dir_all(&project_dir.join("a")).expect("failed to create dir a");
    create_dir_all(&project_dir.join("b")).expect("failed to create dir b");
    write(&project_dir.join("a/shot.png"), b"a").expect("failed to write a/shot.png");
    write(&project_dir.join("b/shot.png"), b"b").expect("failed to write b/shot.png");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let app_manifest = formatdoc! {r#"
        screenshots:
          - a/shot.png
          - b/shot.png
    "#};
    write_string(&project_dir.join("icp_manifest.yaml"), &app_manifest)
        .expect("failed to write app manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("same bundle path").and(contains("shot.png")));
}

/// A screenshot path resolving outside the project directory must be rejected, like other bundle
/// sources.
#[test]
fn bundle_rejects_screenshot_outside_project() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    let outside = project_dir
        .parent()
        .expect("project dir has no parent")
        .join("outside.png");
    write(&outside, b"secret").expect("failed to write outside screenshot");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let app_manifest = formatdoc! {r#"
        screenshots:
          - ../outside.png
    "#};
    write_string(&project_dir.join("icp_manifest.yaml"), &app_manifest)
        .expect("failed to write app manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("outside the project directory"));
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

/// Two canisters whose names sanitize to the same archive segment must be rejected up front,
/// since their archive paths would otherwise collide silently.
#[test]
fn bundle_rejects_canister_name_collision() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    // `my canister` and `my!canister` both sanitize to `my_canister`.
    let pm = formatdoc! {r#"
        canisters:
          - name: "my canister"
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
          - name: "my!canister"
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("sanitize to the same archive segment").and(contains("my_canister")));
}

/// The bundle output path must not live inside a directory that will be recursively archived,
/// otherwise the bundle would include a partial copy of itself.
#[test]
fn bundle_rejects_output_inside_synced_dir() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    let asset_dir = project_dir.join("www");
    create_dir_all(&asset_dir).expect("failed to create asset dir");
    write_string(&asset_dir.join("index.html"), "hello").expect("failed to write asset file");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"
            sync:
              steps:
                - type: assets
                  dir: www
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "www/bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("inside synced directory").and(contains("www")));
}

/// A managed-image network with an absolute bind-mount host path can't be reproduced on
/// another machine, so bundling such a project must be rejected.
#[test]
fn bundle_rejects_absolute_bind_mount() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_src = ctx.make_asset("example_icp_mo.wasm");

    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm_src}' "$ICP_WASM_OUTPUT_PATH"

        networks:
          - name: docker-network
            mode: managed
            image: my-image:latest
            port-mapping:
              - "8080:8080"
            mounts:
              - /etc/host-secrets:/container/secrets
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["project", "bundle", "--output", "bundle.tar.gz"])
        .assert()
        .failure()
        .stderr(contains("docker-network").and(contains("absolute host path")));
}
