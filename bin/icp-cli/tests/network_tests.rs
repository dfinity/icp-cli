mod common;

use crate::common::TestEnv;
use crate::common::guard::ChildGuard;
use predicates::str::contains;
use serial_test::file_serial;
use std::process::Command;

#[test]
#[file_serial]
fn hello() {
    let testenv = TestEnv::new().with_dfx();

    let icp_project_dir = testenv.create_project_dir("icp");

    let icp_path = env!("CARGO_BIN_EXE_icp");
    let mut cmd = Command::new(icp_path);
    cmd.env("HOME", testenv.home_path())
        .current_dir(icp_project_dir)
        .arg("network")
        .arg("run");

    let _child_guard = ChildGuard::spawn(&mut cmd).expect("failed to spawn icp network ");

    testenv.configure_dfx_local_network();

    testenv
        .dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    testenv
        .dfx()
        .arg("new")
        .arg("hello")
        .arg("--type")
        .arg("motoko")
        .arg("--frontend")
        .arg("simple-assets")
        .assert()
        .success();

    let project_dir = testenv.home_path().join("hello");
    testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("deploy")
        .arg("--no-wallet")
        .assert()
        .success();

    testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("canister")
        .arg("call")
        .arg("hello_backend")
        .arg("greet")
        .arg(r#"("test")"#)
        .assert()
        .success()
        .stdout(contains(r#"("Hello, test!")"#));

    let output = testenv
        .dfx()
        .current_dir(&project_dir)
        .arg("canister")
        .arg("id")
        .arg("hello_frontend")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let frontend_canister_id = std::str::from_utf8(&output)
        .expect("stdout was not valid UTF-8")
        .trim();

    let url = format!("http://localhost:8000/sample-asset.txt?canisterId={frontend_canister_id}");
    let response = reqwest::blocking::get(&url)
        .expect("Failed to fetch static asset")
        .text()
        .expect("Failed to read response text");
    assert_eq!(
        response, "This is a sample asset!\n",
        "Static asset content mismatch"
    );
}

#[test]
#[file_serial]
fn network_same_port() {
    let testenv = TestEnv::new().with_dfx();

    let project_dir_a = testenv.create_project_dir("a");
    let project_dir_b = testenv.create_project_dir("b");

    let icp_path = env!("CARGO_BIN_EXE_icp");
    let mut cmd = Command::new(icp_path);
    cmd.env("HOME", testenv.home_path())
        .current_dir(&project_dir_a)
        .arg("network")
        .arg("run");
    let _child_guard = ChildGuard::spawn(&mut cmd).expect("failed to spawn icp network ");

    eprintln!("configure dfx local network");
    testenv.configure_dfx_local_network();

    eprintln!("wait for network healthy");
    testenv
        .dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    eprintln!("second network run attempt");
    testenv
        .icp()
        .current_dir(&project_dir_a)
        .args(["network", "run"])
        .assert()
        .failure()
        .stderr(contains(
            "the local network for this project is already running",
        ));

    eprintln!("second network run attempt in another project");
    testenv
        .icp()
        .current_dir(&project_dir_b)
        .args(["network", "run"])
        .assert()
        .failure()
        .stderr(contains(
            "port 8000 is in use by the local network of the project at",
        ));
}
