use crate::common::TestEnv;
use camino_tempfile::NamedUtf8TempFile;
use icp_fs::fs::write;
use predicates::{
    ord::eq,
    prelude::PredicateBooleanExt,
    str::{PredicateStrExt, contains, starts_with},
};
use serial_test::serial;

mod common;

#[test]
#[serial]
fn canister_create() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        f.path()
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();
}

#[test]
#[serial]
fn canister_create_with_options() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
          create:
            options:
              compute_allocation: 10
              memory_allocation: 4294967296
              freezing_threshold: 2592000
              reserved_cycles_limit: 1000000000000
              wasm_memory_limit: 1073741824
              wasm_memory_threshold: 536870912
        "#,
        f.path()
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();

    // Verify creation options
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 10"))
                .and(contains("Memory allocation: 4_294_967_296"))
                .and(contains("Freezing threshold: 2_592_000"))
                .and(contains("Reserved cycles limit: 1_000_000_000_000"))
                .and(contains("Wasm memory limit: 1_073_741_824"))
                .and(contains("Wasm memory threshold: 536_870_912")),
        );
}

#[test]
#[serial]
fn canister_create_with_options_cmdline_override() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Create temporary file
    let f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
          create:
            options:
              compute_allocation: 10
        "#,
        f.path()
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
            "--compute-allocation",
            "20",
        ])
        .assert()
        .success();

    // Verify creation options
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Compute allocation: 20")),
        );
}

#[test]
#[serial]
fn canister_install() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Build canister
    env.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    // Create canister
    env.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "create",
            "--quiet", // Set quiet so only the canister ID is output
            "--effective-id",
            "ghsi2-tqaaa-aaaan-aaaca-cai",
        ])
        .assert()
        .success();

    // Install canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "install"])
        .assert()
        .success();

    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "call", "my-canister", "greet", "(\"test\")"])
        .assert()
        .success()
        .stdout(eq("(\"Hello, test!\")").trim());
}

#[test]
#[serial]
fn canister_status() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Query status
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae")),
        );
}

#[test]
#[serial]
fn canister_stop() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Stop canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "stop", "my-canister"])
        .assert()
        .success();

    // Query status
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Stopped"))
                .and(contains("Controllers: 2vxsx-fae")),
        );
}

#[test]
#[serial]
fn canister_start() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Stop canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "stop", "my-canister"])
        .assert()
        .success();

    // Query status
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Stopped"))
                .and(contains("Controllers: 2vxsx-fae")),
        );

    // Start canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "start", "my-canister"])
        .assert()
        .success();

    // Query status
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .success()
        .stderr(
            starts_with("Canister Status Report:")
                .and(contains("Status: Running"))
                .and(contains("Controllers: 2vxsx-fae")),
        );
}
#[test]
#[serial]
fn canister_delete() {
    let env = TestEnv::new().with_dfx();

    // Setup project
    let project_dir = env.create_project_dir("icp");

    // Use vendored WASM
    let wasm = env.make_asset("example_icp_mo.wasm");

    // Project manifest
    let pm = format!(
        r#"
        canister:
          name: my-canister
          build:
            adapter:
              type: script
              command: echo {}
        "#,
        wasm,
    );

    write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Start network
    let _g = env.start_network_in(&project_dir);

    // Wait for network
    env.configure_dfx_local_network();

    env.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    // Deploy project
    env.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--effective-id", "ghsi2-tqaaa-aaaan-aaaca-cai"])
        .assert()
        .success();

    // Stop canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "stop", "my-canister"])
        .assert()
        .success();

    // Delete canister
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "delete", "my-canister"])
        .assert()
        .success();

    // Query status
    env.icp()
        .current_dir(&project_dir)
        .args(["canister", "status", "my-canister"])
        .assert()
        .failure();
}
