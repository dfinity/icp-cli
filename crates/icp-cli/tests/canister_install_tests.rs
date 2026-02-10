use indoc::formatdoc;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

#[tokio::test]
async fn canister_install() {
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

    // Build canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--quiet", // Set quiet so only the canister ID is output
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Install canister
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "my-canister",
            "--environment",
            "random-environment",
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
            "greet",
            "(\"test\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[tokio::test]
async fn canister_install_with_valid_principal() {
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
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Valid principal
    let principal = "aaaaa-aa";

    // Try to install with principal (should fail without --wasm flag)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            principal,
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure()
        .stderr(contains(
            "Cannot install canister by principal without --wasm flag",
        ));
}

#[tokio::test]
async fn canister_install_with_wasm_flag() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm_path = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with a different build command that won't produce a valid wasm
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Install canister using --wasm flag
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "my-canister",
            "--wasm",
            wasm_path.as_str(),
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify the installation by calling the canister
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

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_install_with_init_args_candid() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Project manifest with Motoko recipe and init_args
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""
            init_args: "(opt 42 : opt nat8)"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy with init_args
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

    // Verify init arg was set by calling get()
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
async fn canister_install_with_init_args_hex() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Project manifest with init_args in hex format
    // This is the hex encoding of Candid "(opt 100 : opt nat8)" - didc encode '(opt 100 : opt nat8)'
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""
            init_args: "4449444c016e7b01000164"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy with init_args
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

    // Verify init arg was set by calling get()
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

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_install_with_environment_init_args_override() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Project manifest with init_args that gets overridden by environment
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""
            init_args: "(opt 1 : opt nat8)"

        {NETWORK_RANDOM_PORT}

        environments:
          - name: random-environment
            network: random-network
            init_args:
              my-canister: "(opt 200 : opt nat8)"
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy with environment override (should use 200, not 1)
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

    // Verify environment override was used (should be "200", not "1")
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
        .stdout(eq("(\"200\")").trim());
}

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_install_with_invalid_init_args() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Project manifest with invalid init_args
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            recipe:
              type: "@dfinity/motoko@v4.0.0"
              configuration:
                main: main.mo
                args: ""
            init_args: "this is not valid hex or candid"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Build
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // Deploy should fail due to invalid init_args
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
        .failure()
        .stderr(contains("Failed to parse arguments"));
}

#[tokio::test]
async fn canister_install_with_environment_settings_override() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use vendored WASM
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // Project manifest with settings that gets overridden by environment
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
            settings:
              memory_allocation: 1073741824

        {NETWORK_RANDOM_PORT}

        environments:
          - name: random-environment
            network: random-network
            settings:
              my-canister:
                memory_allocation: 2147483648
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Deploy should use the environment override (memory_allocation: 2GB)
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

    // Verify the canister was created with the overridden settings
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "my-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("Memory allocation: 2_147_483_648"),
        "Expected memory_allocation to be 2_147_483_648 (2GB) from environment override, got: {}",
        output_str
    );
}

#[cfg(unix)] // requires bash and wasm-tools in PATH
#[tokio::test]
async fn canister_install_large_wasm_chunked() {
    // Generate large.wasm which is greater than 3MB
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("assets");
    std::process::Command::new("bash")
        .current_dir(&assets_dir)
        .arg("generate_large_wasm.sh")
        .status()
        .expect("failed to run generate_large_wasm.sh");

    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Use the 3MB wasm file to test chunked installation
    let wasm = ctx.make_asset("large.wasm");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - name: large-canister
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

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

    // Build canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "large-canister"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "large-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Install large canister (should use chunked installation)
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "large-canister",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Verify the installation by checking the canister status
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "--environment",
            "random-environment",
            "large-canister",
        ])
        .assert()
        .success()
        .stdout(contains("Status: Running"));
}

#[cfg(unix)] // moc
#[tokio::test]
async fn canister_install_with_args_from_file() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Copy Motoko canister assets
    ctx.copy_asset_dir("echo_init_arg_canister", &project_dir);

    // Project manifest
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

    // Create argument file with opt (42 : nat8)
    write_string(&project_dir.join("args.txt"), "(opt (42 : nat8))")
        .expect("failed to write args file");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Build canister
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "my-canister"])
        .assert()
        .success();

    // Create canister
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "my-canister",
            "--quiet",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Install canister with args from file
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "install",
            "my-canister",
            "--environment",
            "random-environment",
            "--args",
            "args.txt",
        ])
        .assert()
        .success();

    // Verify the init arg was correctly passed by calling get()
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
async fn deploy_with_init_args_from_file_in_manifest() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create canister directory
    let canister_dir = project_dir.join("my-canister");
    std::fs::create_dir(&canister_dir).expect("failed to create canister dir");

    // Copy Motoko canister source files to the canister directory
    ctx.copy_asset_dir("echo_init_arg_canister", &canister_dir);

    // Create init args file in the canister directory
    write_string(&canister_dir.join("init.txt"), "(opt (42 : nat8))")
        .expect("failed to write init args file");

    // Canister manifest with init_args referencing a file
    let cm = formatdoc! {r#"
        name: my-canister
        init_args: "init.txt"
        recipe:
          type: "@dfinity/motoko@v4.0.0"
          configuration:
            main: main.mo
            args: ""
    "#};

    write_string(&canister_dir.join("canister.yaml"), &cm)
        .expect("failed to write canister manifest");

    // Project manifest
    let pm = formatdoc! {r#"
        canisters:
          - my-canister

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // Start network
    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    // Mint cycles
    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

    // Deploy canister (should read init_args from file)
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

    // Verify the init arg was correctly passed by calling get()
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
