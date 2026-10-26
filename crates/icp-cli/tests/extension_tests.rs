use indoc::formatdoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

use crate::common::{
    ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, build_hello_extension,
};
use icp_project::fs::write_string;
use icp_project::prelude::*;

mod common;

#[test]
fn extension_install_run_and_uninstall() {
    let ctx = TestContext::new();
    let wasm = build_hello_extension();

    // Nobody is at the terminal to approve it.
    ctx.icp()
        .args(["extension", "install", "hello", wasm.as_str()])
        .assert()
        .failure()
        .stderr(contains("Calls as your identity, on mainnet:"))
        .stderr(contains("a ledger: can move your ICP"))
        .stderr(contains("pass --yes or --sha256"));

    ctx.icp()
        .args(["extension", "install", "hello", wasm.as_str(), "--yes"])
        .assert()
        .success()
        .stderr(contains("Installed hello 0.1.0 as `icp hello`"));

    ctx.icp()
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(
            contains("hello")
                .and(contains("0.1.0"))
                .and(contains(wasm.as_str())),
        );

    // Its commands are icp's own: listed in help, parsed by clap.
    ctx.icp()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("hello").and(contains("[extension 0.1.0]")));
    ctx.icp()
        .args(["hello", "greet", "--help"])
        .assert()
        .success()
        .stdout(contains("--shout"));
    ctx.icp()
        .args(["hello", "greet"])
        .assert()
        .success()
        .stdout("hello, world\n");
    ctx.icp()
        .args(["hello", "greet", "Bob", "--shout"])
        .assert()
        .success()
        .stdout("HELLO, BOB!\n");
    ctx.icp()
        .args(["hello", "greet", "--bogus"])
        .assert()
        .failure()
        .stderr(contains("unexpected argument '--bogus'"));
    ctx.icp()
        .args(["hello", "fail"])
        .assert()
        .failure()
        .stderr(contains("deliberate failure"));

    // The standard flags are icp's, with icp's rules.
    ctx.icp()
        .args(["hello", "whoami", "-e", "x", "-n", "ic"])
        .assert()
        .failure()
        .stderr(contains("cannot be used with"));

    ctx.icp()
        .args(["extension", "uninstall", "hello"])
        .assert()
        .success();
    ctx.icp()
        .args(["hello", "greet"])
        .assert()
        .failure()
        .stderr(contains("unrecognized subcommand 'hello'"));
}

#[test]
fn extension_install_checks_the_given_sha256() {
    let ctx = TestContext::new();
    let wasm = build_hello_extension();
    ctx.icp()
        .args([
            "extension",
            "install",
            "hello",
            wasm.as_str(),
            "--sha256",
            "00",
        ])
        .assert()
        .failure()
        .stderr(contains("not 00 as --sha256 requires"));

    let sha256 = icp_app_sha256(&wasm);
    ctx.icp()
        .args([
            "extension",
            "install",
            "hello",
            wasm.as_str(),
            "--sha256",
            &sha256,
        ])
        .assert()
        .success();
}

#[test]
fn extension_changed_after_install_is_refused() {
    let ctx = TestContext::new();
    let wasm = build_hello_extension();
    ctx.icp()
        .args(["extension", "install", "hello", wasm.as_str(), "--yes"])
        .assert()
        .success();

    // Wherever the platform keeps user data.
    let installed =
        find(ctx.home_path(), "extensions/hello/extension.wasm").expect("the installed component");
    let mut bytes = std::fs::read(&installed).unwrap();
    bytes.push(0);
    std::fs::write(&installed, bytes).unwrap();

    ctx.icp()
        .args(["hello", "greet"])
        .assert()
        .failure()
        .stderr(contains("has changed since it was approved"));
}

#[tokio::test]
async fn extension_on_a_local_network() {
    let ctx = TestContext::new();
    let wasm = build_hello_extension();
    ctx.icp()
        .args(["extension", "install", "hello", wasm.as_str(), "--yes"])
        .assert()
        .success();

    let project_dir = ctx.create_project_dir("icp");
    let canister_wasm = ctx.make_asset("example_icp_mo.wasm");
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: cp '{canister_wasm}' "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");
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

    // The project's canister IDs reach the extension.
    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["hello", "whoami", "--environment", "random-environment"])
        .assert()
        .success()
        .stdout(contains("mainnet=false"))
        .stdout(contains("environment=random-environment"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let id = stdout
        .lines()
        .find_map(|line| line.strip_prefix("canister my-canister="))
        .expect("the canister's ID")
        .to_owned();

    assert!(!id.is_empty());

    // Off mainnet, a canister the extension never declared can be called:
    // here the cycles ledger.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "hello",
            "call",
            "um5iw-rqaaa-aaaaq-qaaba-cai",
            "icrc1_name",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("reply"));
}

fn icp_app_sha256(path: &camino::Utf8Path) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(std::fs::read(path).unwrap()))
}

fn find(dir: &Path, suffix: &str) -> Option<PathBuf> {
    for entry in dir.read_dir_utf8().ok()?.flatten() {
        let path = entry.path().to_owned();
        if path.ends_with(suffix) {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find(&path, suffix)
        {
            return Some(found);
        }
    }
    None
}

#[test]
fn extension_installed_under_another_name() {
    let ctx = TestContext::new();
    let wasm = build_hello_extension();
    for name in ["hello", "hello-beta"] {
        ctx.icp()
            .args(["extension", "install", name, wasm.as_str(), "--yes"])
            .assert()
            .success()
            .stderr(contains(format!("Install as  icp {name}")));
    }

    // The name it was installed as is the command; the extension's own is
    // only shown.
    ctx.icp()
        .args(["hello-beta", "greet", "Ann"])
        .assert()
        .success()
        .stdout("hello, Ann\n");
    ctx.icp()
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(contains("hello ").and(contains("hello-beta ")));
    ctx.icp()
        .args(["extension", "show", "hello-beta"])
        .assert()
        .success()
        .stdout(contains("Name:       hello-beta"))
        .stdout(contains("Extension:  hello 0.1.0"))
        .stdout(contains("icp hello-beta greet"));

    // Uninstalling one leaves the other.
    ctx.icp()
        .args(["extension", "uninstall", "hello-beta"])
        .assert()
        .success();
    ctx.icp().args(["hello", "greet"]).assert().success();
}

#[test]
fn extension_show_json() {
    let ctx = TestContext::new();
    let wasm = build_hello_extension();
    ctx.icp()
        .args(["extension", "install", "hello", wasm.as_str(), "--yes"])
        .assert()
        .success();

    let output = ctx
        .icp()
        .args(["extension", "show", "hello", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let shown: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(shown["name"], "hello");
    assert_eq!(shown["extension"], "hello");
    assert_eq!(shown["version"], "0.1.0");
    assert_eq!(shown["source"], wasm.as_str());
    assert_eq!(shown["sha256"], icp_app_sha256(&wasm));
    assert_eq!(shown["intact"], true);
    assert!(shown["installed_at"].as_str().unwrap().starts_with("20"));
    assert_eq!(shown["canisters"][0]["id"], "ryjl3-tyaaa-aaaaa-aaaba-cai");
    assert!(
        shown["commands"]
            .as_array()
            .unwrap()
            .contains(&"hello greet".into())
    );

    let output = ctx
        .icp()
        .args(["extension", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(listed[0]["name"], "hello");

    ctx.icp()
        .args(["extension", "show", "nope"])
        .assert()
        .failure()
        .stderr(contains("no extension named 'nope' is installed"));
}

#[test]
fn extension_install_checks_the_name_first() {
    let ctx = TestContext::new();
    // Neither is downloaded: the name is refused before the source is read.
    ctx.icp()
        .args([
            "extension",
            "install",
            "canister",
            "https://example.invalid/x.wasm",
        ])
        .assert()
        .failure()
        .stderr(contains("'canister' is a built-in icp command"));
    ctx.icp()
        .args([
            "extension",
            "install",
            "../x",
            "https://example.invalid/x.wasm",
        ])
        .assert()
        .failure()
        .stderr(contains("not a valid command name"));
}
