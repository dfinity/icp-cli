mod common;

use std::env::current_dir;
use std::path::PathBuf;
use std::process::Command;

use crate::common::TestEnv;
use predicates::str::contains;

#[test]
fn hello() {
    let testenv = TestEnv::new().with_dfx();

    let icp_project_dir = testenv.home_path().join("icp");
    std::fs::create_dir_all(&icp_project_dir).expect("Failed to create icp project directory");
    std::fs::write(icp_project_dir.join("icp-project.yaml"), "")
        .expect("Failed to write project file");

    let icp_path = env!("CARGO_BIN_EXE_icp");
    let mut cmd = Command::new(icp_path);
    cmd.env("HOME", testenv.home_path());

    let mut child= cmd
        .current_dir(icp_project_dir)
        .arg("network")
        .arg("run")
        .spawn()
        .expect("failed to spawn icp network");

    struct ChildGuard {
        child: std::process::Child,
    }
    impl ChildGuard {
        fn new(child: std::process::Child) -> Self {
            Self { child }
        }
    }
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Err(e) = self.child.kill() {
                eprintln!("Failed to kill child process: {}", e);
            }
            if let Some(code) = self.child.wait().ok().and_then(|status| status.code()) {
                eprintln!("Child process exited with code: {}", code);
            } else {
                eprintln!("Child process terminated unexpectedly");
            }
        }
    }

    let _child_guard = ChildGuard::new(child);

    // configure the network for dfx
    testenv.configure_dfx_local_network();

    testenv.dfx()
        .arg("ping")
        .arg("--wait-healthy")
        .assert()
        .success();

    eprintln!("***** call dfx new *****");
    testenv.dfx()
        .arg("new")
        .arg("hello")
        .arg("--type")
        .arg("motoko")
        .arg("--frontend")
        .arg("simple-assets")
        .assert()
        .success();

    eprintln!("***** call dfx deploy *****");

    let project_dir = testenv.home_path().join("hello");
    testenv.dfx_in_directory(&project_dir)
        .arg("deploy")
        .arg("--no-wallet")
        .assert()
        .success();

    testenv.dfx_in_directory(&project_dir)
        .arg("canister")
        .arg("call")
        .arg("hello_backend")
        .arg("greet")
        .arg(r#"("test")"#)
        .assert()
        .success()
        .stdout(contains(r#"("Hello, test!")"#));

    let output = testenv.dfx_in_directory(&project_dir)
        .arg("canister")
        .arg("id")
        .arg("hello_frontend")
        .assert()
        .success()
        .get_output()
        .stdout.clone();

    let frontend_canister_id = std::str::from_utf8(&output)
        .expect("stdout was not valid UTF-8")
        .trim();

    let url = format!("http://localhost:8000/sample-asset.txt?canisterId={}", frontend_canister_id);
    eprintln!("***** call frontend URL: {} *****", url);
    let response = reqwest::blocking::get(&url)
        .expect("Failed to fetch static asset")
        .text()
        .expect("Failed to read response text");
    eprintln!("***** response: {} *****", response);
    assert_eq!(response, "This is a sample asset!\n", "Static asset content mismatch");
}
