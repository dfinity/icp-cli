use core::str;
use std::io::Write;

use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use common::TestContext;
use ic_agent::export::Principal;
use icp::{fs::write_string, prelude::*};
use indoc::formatdoc;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, clients};

mod common;

#[test]
fn identity_anonymous() {
    let ctx = TestContext::new();
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(eq("* anonymous 2vxsx-fae").trim());
    ctx.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("2vxsx-fae").trim());
}

#[test]
fn identity_import_seed() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"equip will roof matter pink blind book anxiety banner elbow sun young")
        .unwrap();
    let path = file.into_temp_path();
    let ctx = TestContext::new();
    ctx.icp()
        .args(["identity", "import", "alice", "--from-seed-file"])
        .arg(&path)
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("alice"))
        .stdout(contains("anonymous"));
    ctx.icp()
        .args(["identity", "default", "alice"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[test]
fn identity_import_pem() {
    // from plaintext sec1
    let ctx = TestContext::new();
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
    ctx.icp()
        .args(["identity", "import", "alice2", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_p256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice2"])
        .assert()
        .success()
        .stdout(eq("qkn3n-adewz-qvqxz-gkchb-ughyj-pl23l-ezdak-7rnds-fime4-si4tn-nae").trim());
    ctx.icp()
        .args(["identity", "import", "alice3", "--from-pem"])
        .arg(ctx.make_asset("decrypted_pkcs8_ed25519.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice3"])
        .assert()
        .success()
        .stdout(eq("jj5yb-kxwog-t6hmv-jxmpm-rtuci-uikjz-qkezj-4armi-wihgt-ulmi3-bqe").trim());

    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("missing_params_sec1_k256.pem"))
        .assert()
        .failure()
        .stderr(contains("missing field `parameters`"));
    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("unsupported_curve_sec1.pem"))
        .assert()
        .failure()
        .stderr(contains("unsupported algorithm"));

    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("separate_params_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "bob"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
    ctx.icp()
        .args(["identity", "import", "bob2", "--from-pem"])
        .arg(ctx.make_asset("separate_params_sec1_p256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "bob2"])
        .assert()
        .success()
        .stdout(eq("qkn3n-adewz-qvqxz-gkchb-ughyj-pl23l-ezdak-7rnds-fime4-si4tn-nae").trim());

    ctx.icp()
        .args(["identity", "import", "carol", "--from-pem"])
        .arg(ctx.make_asset("missing_params_sec1_k256.pem"))
        .args(["--assert-key-type", "secp256k1"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "carol"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    // from encrypted pkcs8
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"swordfish").unwrap();
    let path = file.into_temp_path();
    ctx.icp()
        .args(["identity", "import", "chlöe", "--from-pem"])
        .arg(ctx.make_asset("encrypted_pkcs8_k256.pem"))
        .arg("--decryption-password-from-file")
        .arg(&path)
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "chlöe"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    // from plaintext pkcs8
    ctx.icp()
        .args(["identity", "import", "d'artagnan", "--from-pem"])
        .arg(ctx.make_asset("pkcs8_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "d'artagnan"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
    ctx.icp()
        .args(["identity", "import", "d'artagnan2", "--from-pem"])
        .arg(ctx.make_asset("pkcs8_p256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "d'artagnan2"])
        .assert()
        .success()
        .stdout(eq("qkn3n-adewz-qvqxz-gkchb-ughyj-pl23l-ezdak-7rnds-fime4-si4tn-nae").trim());
}

#[test]
fn identity_create() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let new_out = ctx
        .icp()
        .args(["identity", "new", "alice"])
        .assert()
        .success();

    let alice_principal = clients::icp(&ctx, &project_dir, None).get_principal("alice");
    let anonymous_principal = clients::icp(&ctx, &project_dir, None).get_principal("anonymous");

    let seed = str::from_utf8(&new_out.get_output().stdout)
        .unwrap()
        .strip_prefix("Your seed phrase: ")
        .unwrap();

    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains(format!("alice     {alice_principal}")))
        .stdout(contains(format!("anonymous {anonymous_principal}")));

    let principal1_out = ctx
        .icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success();

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(seed.trim().as_bytes()).unwrap();
    let path = file.into_temp_path();

    ctx.icp()
        .args(["identity", "import", "bob", "--from-seed-file"])
        .arg(&path)
        .assert()
        .success();
    let principal2_out = ctx
        .icp()
        .args(["identity", "principal", "--identity", "bob"])
        .assert()
        .success();
    let principal1 = str::from_utf8(&principal1_out.get_output().stdout)
        .unwrap()
        .trim()
        .parse::<Principal>()
        .unwrap();
    let principal2 = str::from_utf8(&principal2_out.get_output().stdout)
        .unwrap()
        .trim()
        .parse::<Principal>()
        .unwrap();

    assert_eq!(principal1, principal2);
}

#[test]
fn identity_use() {
    let ctx = TestContext::new();
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "default", "alice"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
    ctx.icp()
        .args(["identity", "principal", "--identity", "anonymous"])
        .assert()
        .success()
        .stdout(eq("2vxsx-fae").trim());
    ctx.icp()
        .args(["identity", "default", "anonymous"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("2vxsx-fae").trim());
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[tokio::test]
async fn identity_storage_forms() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Create password file for the password-protected identity storage
    let mut storage_password_file = NamedTempFile::new().unwrap();
    storage_password_file
        .write_all(b"test-password-123")
        .unwrap();
    let storage_password_path = storage_password_file.into_temp_path();

    ctx.icp()
        .args(["identity", "new", "id_plaintext", "--storage", "plaintext"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "new", "id_keyring", "--storage", "keyring"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "new", "id_password", "--storage", "password"])
        .arg("--storage-password-file")
        .arg(&storage_password_path)
        .assert()
        .success();

    ctx.icp()
        .args(["identity", "principal", "--identity", "id_plaintext"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "id_keyring"])
        .assert()
        .success();
    ctx.icp()
        .arg("--identity-password-file")
        .arg(&storage_password_path)
        .args(["identity", "principal", "--identity", "id_password"])
        .assert()
        .success();

    // Set up project with greeter canister
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let pm = formatdoc! {r#"
        canisters:
          - name: greeter
            build:
              steps:
                - type: script
                  command: cp {wasm} "$ICP_WASM_OUTPUT_PATH"

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network");
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(10 * TRILLION);

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

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--identity",
            "id_plaintext",
            "greeter",
            "greet",
            "(\"plaintext\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, plaintext!\")").trim());
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--identity",
            "id_keyring",
            "greeter",
            "greet",
            "(\"keyring\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, keyring!\")").trim());
    ctx.icp()
        .current_dir(&project_dir)
        .arg("--identity-password-file")
        .arg(&storage_password_path)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--identity",
            "id_password",
            "greeter",
            "greet",
            "(\"password\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, password!\")").trim());
}
