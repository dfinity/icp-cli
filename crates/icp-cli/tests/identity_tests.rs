use core::str;
use std::io::Write;

use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use common::TestContext;
use ic_agent::export::Principal;
use icp::{fs::write_string, prelude::*};
use indoc::formatdoc;
use predicates::{ord::eq, prelude::*, str::contains};

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
fn identity_import_seed_curve() {
    // Seed: "equip will roof matter pink blind book anxiety banner elbow sun young"
    // p256:   SLIP-0010 "Nist256p1 seed", path m/44'/223'/0'/0/0
    // ed25519: SLIP-0010 "ed25519 seed",  path m/44'/223'/0'/0'/0'
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"equip will roof matter pink blind book anxiety banner elbow sun young")
        .unwrap();
    let path = file.into_temp_path();
    let ctx = TestContext::new();

    ctx.icp()
        .args(["identity", "import", "alice_p256", "--from-seed-file"])
        .arg(&path)
        .args(["--seed-curve", "prime256v1"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice_p256"])
        .assert()
        .success()
        .stdout(eq("gu6g3-gzs4p-fyjio-reppd-qk7ef-lhput-eg36s-ofyim-gi6y4-ce3qs-zqe").trim());

    ctx.icp()
        .args(["identity", "import", "alice_ed25519", "--from-seed-file"])
        .arg(&path)
        .args(["--seed-curve", "ed25519"])
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice_ed25519"])
        .assert()
        .success()
        .stdout(eq("z2yk5-gbsi4-5eudl-y5q6u-qaqmf-37gjy-r66iy-oiqvb-d5nbr-5odxa-4qe").trim());
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

    // from nonconforming pkcs8 generated by old dfx versions
    ctx.icp()
        .args(["identity", "import", "eve", "--from-pem"])
        .arg(ctx.make_asset("broken_dfx_pkcs8_ed25519.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "eve"])
        .assert()
        .success()
        .stdout(eq("x7ywp-7tq2e-kf2va-55mam-af7m6-5hvrz-apafo-ytb7z-fsuh2-qdo4s-nqe").trim());
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
        .lines()
        .next_back()
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

    // Reject password shorter than 8 characters
    let mut short_password_file = NamedTempFile::new().unwrap();
    short_password_file.write_all(b"1234567").unwrap();
    let short_password_path = short_password_file.into_temp_path();
    ctx.icp()
        .args(["identity", "new", "id_short_pw", "--storage", "password"])
        .arg("--storage-password-file")
        .arg(&short_password_path)
        .assert()
        .failure()
        .stderr(contains("password must be at least 8 characters"));

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
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

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

#[test]
fn identity_account_id() {
    let ctx = TestContext::new();

    // Test account-id for anonymous identity (default in test context)
    ctx.icp()
        .args(["identity", "account-id"])
        .assert()
        .success()
        .stdout(
            contains("1c7a48ba6a562aa9eaa2481a9049cdf0433b9738c992d698c31d8abf89cadc79").trim(),
        );

    // Test account-id with --of-principal flag
    ctx.icp()
        .args(["identity", "account-id", "--of-principal", "aaaaa-aa"])
        .assert()
        .success()
        .stdout(
            contains("2d0e897f7e862d2b57d9bc9ea5c65f9a24ac6c074575f47898314b8d6cb0929d").trim(),
        );

    // Import an identity and test its account-id
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "account-id", "--identity", "alice"])
        .assert()
        .success()
        .stdout(
            contains("4f3d4b40cdb852732601fccf8bd24dffe44957a647cb867913e982d98cf85676").trim(),
        );
}

#[test]
fn identity_rename() {
    let ctx = TestContext::new();

    // Import an identity to rename
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Get the principal before rename
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    // Rename the identity
    ctx.icp()
        .args(["identity", "rename", "alice", "bob"])
        .assert()
        .success()
        .stderr(contains("Renamed identity `alice` to `bob`"));

    // Verify old name no longer exists
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .failure()
        .stderr(contains("no identity found with name `alice`"));

    // Verify new name works and has the same principal
    ctx.icp()
        .args(["identity", "principal", "--identity", "bob"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    // Verify list shows the new name
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("bob"))
        .stdout(predicates::str::contains("alice").not());
}

#[test]
fn identity_rename_updates_default() {
    let ctx = TestContext::new();

    // Import and set as default
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "default", "alice"])
        .assert()
        .success();

    // Verify alice is the default
    ctx.icp()
        .args(["identity", "default"])
        .assert()
        .success()
        .stdout(eq("alice").trim());

    // Rename alice to bob
    ctx.icp()
        .args(["identity", "rename", "alice", "bob"])
        .assert()
        .success();

    // Verify default was updated to bob
    ctx.icp()
        .args(["identity", "default"])
        .assert()
        .success()
        .stdout(eq("bob").trim());
}

#[test]
fn identity_rename_anonymous() {
    let ctx = TestContext::new();

    // Cannot rename from anonymous
    ctx.icp()
        .args(["identity", "rename", "anonymous", "not-anonymous"])
        .assert()
        .failure()
        .stderr(contains("cannot rename the anonymous identity"));

    // Import an identity to try renaming to anonymous
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Cannot rename to anonymous
    ctx.icp()
        .args(["identity", "rename", "alice", "anonymous"])
        .assert()
        .failure()
        .stderr(contains("cannot rename to the anonymous identity"));
}

#[test]
fn identity_rename_target_already_exists() {
    let ctx = TestContext::new();

    // Import two identities
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_p256.pem"))
        .assert()
        .success();

    // Try to rename alice to bob (which already exists)
    ctx.icp()
        .args(["identity", "rename", "alice", "bob"])
        .assert()
        .failure()
        .stderr(contains("identity `bob` already exists"));
}

#[test]
fn identity_rename_source_not_found() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["identity", "rename", "nonexistent", "newname"])
        .assert()
        .failure()
        .stderr(contains("no identity found with name `nonexistent`"));
}

#[test]
fn identity_delete() {
    let ctx = TestContext::new();

    // Import an identity to delete
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Verify it exists
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("alice"));

    // Delete it
    ctx.icp()
        .args(["identity", "delete", "alice"])
        .assert()
        .success()
        .stderr(contains("Deleted identity `alice`"));

    // Verify it's gone
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("alice").not());

    // Verify we can't use it
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .failure()
        .stderr(contains("no identity found with name `alice`"));
}

#[test]
fn identity_delete_cannot_delete_anonymous() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["identity", "delete", "anonymous"])
        .assert()
        .failure()
        .stderr(contains("cannot delete the anonymous identity"));
}

#[test]
fn identity_delete_cannot_delete_default() {
    let ctx = TestContext::new();

    // Import and set as default
    ctx.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "default", "alice"])
        .assert()
        .success();

    // Try to delete it
    ctx.icp()
        .args(["identity", "delete", "alice"])
        .assert()
        .failure()
        .stderr(contains("cannot delete the default identity"));

    // Change the default to anonymous
    ctx.icp()
        .args(["identity", "default", "anonymous"])
        .assert()
        .success();

    // Now deletion should work
    ctx.icp()
        .args(["identity", "delete", "alice"])
        .assert()
        .success();
}

#[test]
fn identity_delete_not_found() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["identity", "delete", "nonexistent"])
        .assert()
        .failure()
        .stderr(contains("no identity found with name `nonexistent`"));
}

#[test]
fn identity_export_plaintext() {
    let ctx = TestContext::new();

    // Import a plaintext identity
    ctx.icp()
        .args([
            "identity",
            "import",
            "alice",
            "--storage",
            "plaintext",
            "--from-pem",
        ])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Export it and verify the output is valid PEM
    let output = ctx
        .icp()
        .args(["identity", "export", "alice"])
        .assert()
        .success();

    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();

    // Verify it's a valid PEM file
    assert!(stdout.contains("-----BEGIN PRIVATE KEY-----"));
    assert!(stdout.contains("-----END PRIVATE KEY-----"));

    // Verify we can parse it
    let pem = pem::parse(stdout.trim()).expect("should be valid PEM");
    assert_eq!(pem.tag(), "PRIVATE KEY");
}

#[test]
fn identity_export_encrypted_with_password_file() {
    let ctx = TestContext::new();

    // Create a password file
    let password_file = ctx.home_path().join("password.txt");
    std::fs::write(&password_file, "test-password").unwrap();

    // Import an encrypted identity
    ctx.icp()
        .args([
            "identity",
            "import",
            "alice",
            "--storage",
            "password",
            "--storage-password-file",
        ])
        .arg(&password_file)
        .args(["--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Export it with password file
    let output = ctx
        .icp()
        .args(["identity", "export", "alice", "--password-file"])
        .arg(&password_file)
        .assert()
        .success();

    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();

    // Verify it's a valid plaintext PEM file (not encrypted)
    assert!(stdout.contains("-----BEGIN PRIVATE KEY-----"));
    assert!(stdout.contains("-----END PRIVATE KEY-----"));
    assert!(!stdout.contains("-----BEGIN ENCRYPTED PRIVATE KEY-----"));

    // Verify we can parse it
    let pem = pem::parse(stdout.trim()).expect("should be valid PEM");
    assert_eq!(pem.tag(), "PRIVATE KEY");
}

#[test]
fn identity_export_keyring() {
    let ctx = TestContext::new();

    // Import a keyring identity
    ctx.icp()
        .args([
            "identity",
            "import",
            "alice",
            "--storage",
            "keyring",
            "--from-pem",
        ])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Export it
    let output = ctx
        .icp()
        .args(["identity", "export", "alice"])
        .assert()
        .success();

    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();

    // Verify it's a valid PEM file
    assert!(stdout.contains("-----BEGIN PRIVATE KEY-----"));
    assert!(stdout.contains("-----END PRIVATE KEY-----"));

    // Verify we can parse it
    let pem = pem::parse(stdout.trim()).expect("should be valid PEM");
    assert_eq!(pem.tag(), "PRIVATE KEY");

    // Clean up keyring entry
    let _ = ctx.icp().args(["identity", "delete", "alice"]).assert();
}

#[test]
fn identity_export_encrypted_rejects_short_password() {
    let ctx = TestContext::new();
    ctx.icp()
        .args(["identity", "new", "bob", "--storage", "plaintext"])
        .assert()
        .success();
    let mut short_pw = NamedTempFile::new().unwrap();
    short_pw.write_all(b"1234567").unwrap();
    let short_pw_path = short_pw.into_temp_path();
    ctx.icp()
        .args(["identity", "export", "bob", "--encrypt"])
        .arg("--encryption-password-file")
        .arg(&short_pw_path)
        .assert()
        .failure()
        .stderr(contains("password must be at least 8 characters"));
}

#[test]
fn identity_export_encrypted() {
    let ctx = TestContext::new();

    // Import a plaintext identity
    ctx.icp()
        .args([
            "identity",
            "import",
            "alice",
            "--storage",
            "plaintext",
            "--from-pem",
        ])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    let password_file = ctx.home_path().join("encrypt-password.txt");
    std::fs::write(&password_file, "export-password-123").unwrap();

    // Export with encryption
    let output = ctx
        .icp()
        .args([
            "identity",
            "export",
            "alice",
            "--encrypt",
            "--encryption-password-file",
        ])
        .arg(&password_file)
        .assert()
        .success();

    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();

    // Verify it's an encrypted PEM
    assert!(stdout.contains("-----BEGIN ENCRYPTED PRIVATE KEY-----"));
    assert!(stdout.contains("-----END ENCRYPTED PRIVATE KEY-----"));
    assert!(!stdout.contains("-----BEGIN PRIVATE KEY-----"));

    // Re-import the encrypted export and verify same principal
    let export_file = ctx.home_path().join("exported-encrypted.pem");
    std::fs::write(&export_file, stdout).unwrap();

    ctx.icp()
        .args([
            "identity",
            "import",
            "alice-reimported",
            "--storage",
            "plaintext",
            "--from-pem",
        ])
        .arg(&export_file)
        .arg("--decryption-password-from-file")
        .arg(&password_file)
        .assert()
        .success();

    ctx.icp()
        .args(["identity", "principal", "--identity", "alice-reimported"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[test]
fn identity_export_cannot_export_anonymous() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["identity", "export", "anonymous"])
        .assert()
        .failure()
        .stderr(contains("cannot export the anonymous identity"));
}

#[test]
fn identity_export_not_found() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["identity", "export", "nonexistent"])
        .assert()
        .failure()
        .stderr(contains("no identity found with name `nonexistent`"));
}

#[test]
fn identity_export_verifies_principal_unchanged() {
    let ctx = TestContext::new();

    // Import an identity
    ctx.icp()
        .args([
            "identity",
            "import",
            "alice",
            "--storage",
            "plaintext",
            "--from-pem",
        ])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Get the principal before export
    let principal_before = ctx
        .icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success();
    let principal_before_str = std::str::from_utf8(&principal_before.get_output().stdout)
        .unwrap()
        .trim();

    // Export the identity
    let output = ctx
        .icp()
        .args(["identity", "export", "alice"])
        .assert()
        .success();

    let exported_pem = std::str::from_utf8(&output.get_output().stdout).unwrap();

    // Save exported PEM to a temp file
    let export_file = ctx.home_path().join("exported.pem");
    std::fs::write(&export_file, exported_pem).unwrap();

    // Import the exported PEM as a new identity
    ctx.icp()
        .args([
            "identity",
            "import",
            "alice-reimported",
            "--storage",
            "plaintext",
            "--from-pem",
        ])
        .arg(&export_file)
        .assert()
        .success();

    // Get the principal of the reimported identity
    let principal_after = ctx
        .icp()
        .args(["identity", "principal", "--identity", "alice-reimported"])
        .assert()
        .success();
    let principal_after_str = std::str::from_utf8(&principal_after.get_output().stdout)
        .unwrap()
        .trim();

    // Verify principals match
    assert_eq!(principal_before_str, principal_after_str);
}

#[tokio::test]
async fn identity_link_hsm() {
    let ctx = TestContext::new();
    let hsm = ctx.init_softhsm();
    let project_dir = ctx.create_project_dir("icp");

    // Create a PIN file for non-interactive testing
    let pin_file = ctx.home_path().join("pin.txt");
    std::fs::write(&pin_file, &hsm.user_pin).unwrap();

    // Link the HSM key to an identity
    ctx.icp()
        .args(["identity", "link", "hsm", "hsm-identity"])
        .args(["--pkcs11-module", hsm.library_path_str()])
        .args(["--slot", &hsm.slot_index.to_string()])
        .args(["--key-id", &hsm.key_id])
        .arg("--pin-file")
        .arg(&pin_file)
        .assert()
        .success()
        .stderr(contains("Identity `hsm-identity` linked to HSM"));

    // Verify the identity appears in the list
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("hsm-identity"));

    // Verify we can get the principal
    let principal_output = ctx
        .icp()
        .arg("--identity-password-file")
        .arg(&pin_file)
        .args(["identity", "principal", "--identity", "hsm-identity"])
        .assert()
        .success();

    let principal_str = std::str::from_utf8(&principal_output.get_output().stdout)
        .unwrap()
        .trim();

    // Verify the principal is valid (not anonymous, not empty)
    assert!(!principal_str.is_empty());
    assert_ne!(principal_str, "2vxsx-fae"); // Not anonymous
    principal_str.parse::<Principal>().expect("valid principal");

    // Set up project with greeter canister to verify signing works
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let pm = formatdoc! {r#"
        canisters:
          - name: greeter
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

    // Call the canister with the HSM identity to verify signing works
    ctx.icp()
        .current_dir(&project_dir)
        .arg("--identity-password-file")
        .arg(&pin_file)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--identity",
            "hsm-identity",
            "greeter",
            "greet",
            "(\"hsm\")",
        ])
        .assert()
        .success()
        .stdout(eq("(\"Hello, hsm!\")").trim());
}

#[test]
fn identity_link_hsm_already_exists() {
    let ctx = TestContext::new();
    let hsm = ctx.init_softhsm();

    let pin_file = ctx.home_path().join("pin.txt");
    std::fs::write(&pin_file, &hsm.user_pin).unwrap();

    // Link once
    ctx.icp()
        .args(["identity", "link", "hsm", "hsm-identity"])
        .args(["--pkcs11-module", hsm.library_path_str()])
        .args(["--slot", &hsm.slot_index.to_string()])
        .args(["--key-id", &hsm.key_id])
        .arg("--pin-file")
        .arg(&pin_file)
        .assert()
        .success();

    // Try to link again with the same name
    ctx.icp()
        .args(["identity", "link", "hsm", "hsm-identity"])
        .args(["--pkcs11-module", hsm.library_path_str()])
        .args(["--slot", &hsm.slot_index.to_string()])
        .args(["--key-id", &hsm.key_id])
        .arg("--pin-file")
        .arg(&pin_file)
        .assert()
        .failure()
        .stderr(contains("identity `hsm-identity` already exists"));
}

#[test]
fn identity_link_hsm_cannot_export() {
    let ctx = TestContext::new();
    let hsm = ctx.init_softhsm();

    let pin_file = ctx.home_path().join("pin.txt");
    std::fs::write(&pin_file, &hsm.user_pin).unwrap();

    // Link the identity
    ctx.icp()
        .args(["identity", "link", "hsm", "hsm-identity"])
        .args(["--pkcs11-module", hsm.library_path_str()])
        .args(["--slot", &hsm.slot_index.to_string()])
        .args(["--key-id", &hsm.key_id])
        .arg("--pin-file")
        .arg(&pin_file)
        .assert()
        .success();

    // Try to export - should fail
    ctx.icp()
        .args(["identity", "export", "hsm-identity"])
        .assert()
        .failure()
        .stderr(contains("cannot export an HSM-backed identity"));
}

#[test]
fn identity_link_hsm_rename() {
    let ctx = TestContext::new();
    let hsm = ctx.init_softhsm();

    let pin_file = ctx.home_path().join("pin.txt");
    std::fs::write(&pin_file, &hsm.user_pin).unwrap();

    // Link the identity
    ctx.icp()
        .args(["identity", "link", "hsm", "hsm-identity"])
        .args(["--pkcs11-module", hsm.library_path_str()])
        .args(["--slot", &hsm.slot_index.to_string()])
        .args(["--key-id", &hsm.key_id])
        .arg("--pin-file")
        .arg(&pin_file)
        .assert()
        .success();

    // Get principal before rename
    let principal_before = ctx
        .icp()
        .arg("--identity-password-file")
        .arg(&pin_file)
        .args(["identity", "principal", "--identity", "hsm-identity"])
        .assert()
        .success();
    let principal_before_str = std::str::from_utf8(&principal_before.get_output().stdout)
        .unwrap()
        .trim();

    // Rename the identity
    ctx.icp()
        .args(["identity", "rename", "hsm-identity", "hsm-renamed"])
        .assert()
        .success();

    // Verify old name no longer exists
    ctx.icp()
        .args(["identity", "principal", "--identity", "hsm-identity"])
        .assert()
        .failure();

    // Verify new name works with same principal
    let principal_after = ctx
        .icp()
        .arg("--identity-password-file")
        .arg(&pin_file)
        .args(["identity", "principal", "--identity", "hsm-renamed"])
        .assert()
        .success();
    let principal_after_str = std::str::from_utf8(&principal_after.get_output().stdout)
        .unwrap()
        .trim();

    assert_eq!(principal_before_str, principal_after_str);
}

#[cfg(unix)] // moc
#[tokio::test]
async fn identity_delegation_whoami() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    // Import a root identity to sign the delegation
    ctx.icp()
        .args(["identity", "import", "root-identity", "--from-pem"])
        .arg(ctx.make_asset("decrypted_sec1_k256.pem"))
        .assert()
        .success();

    // Create a pending delegation identity, capturing the session public key PEM
    let request_output = ctx
        .icp()
        .args([
            "identity",
            "delegation",
            "request",
            "delegated-identity",
            "--storage",
            "plaintext",
        ])
        .assert()
        .success();
    let pem_str = str::from_utf8(&request_output.get_output().stdout).unwrap();

    // Write the session public key PEM to a temp file for the sign step
    let key_pem_file = ctx.home_path().join("session-key.pem");
    std::fs::write(&key_pem_file, pem_str).unwrap();

    // Sign a delegation from root-identity to the session key
    let sign_output = ctx
        .icp()
        .args([
            "identity",
            "delegation",
            "sign",
            "--identity",
            "root-identity",
            "--key-pem",
        ])
        .arg(&key_pem_file)
        .args(["--duration", "1d"])
        .assert()
        .success();
    let chain_json = str::from_utf8(&sign_output.get_output().stdout).unwrap();

    // Write the delegation chain JSON to a temp file for the use step
    let chain_json_file = ctx.home_path().join("delegation-chain.json");
    std::fs::write(&chain_json_file, chain_json).unwrap();

    // Complete the delegation identity with the signed chain
    ctx.icp()
        .args([
            "identity",
            "delegation",
            "use",
            "delegated-identity",
            "--from-json",
        ])
        .arg(&chain_json_file)
        .assert()
        .success();

    // Both identities should present the same principal: the root's principal
    // (delegation chains are rooted at the signing key)
    let root_principal = str::from_utf8(
        &ctx.icp()
            .args(["identity", "principal", "--identity", "root-identity"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let delegated_principal = str::from_utf8(
        &ctx.icp()
            .args(["identity", "principal", "--identity", "delegated-identity"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    assert_eq!(root_principal, delegated_principal);

    // Set up project manifest with whoami canister built via Motoko recipe
    ctx.copy_asset_dir("whoami_canister", &project_dir);
    let pm = formatdoc! {r#"
        canisters:
          - name: whoami
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
            "--subnet",
            common::SUBNET_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    // Call whoami with root-identity
    let root_whoami = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--identity",
            "root-identity",
            "whoami",
            "whoami",
            "()",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    // Call whoami with delegated-identity — the canister sees the root's principal
    let delegated_whoami = ctx
        .icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "call",
            "--environment",
            "random-environment",
            "--identity",
            "delegated-identity",
            "whoami",
            "whoami",
            "()",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(root_whoami, delegated_whoami);
}

#[test]
fn identity_link_hsm_delete() {
    let ctx = TestContext::new();
    let hsm = ctx.init_softhsm();

    let pin_file = ctx.home_path().join("pin.txt");
    std::fs::write(&pin_file, &hsm.user_pin).unwrap();

    // Link the identity
    ctx.icp()
        .args(["identity", "link", "hsm", "hsm-identity"])
        .args(["--pkcs11-module", hsm.library_path_str()])
        .args(["--slot", &hsm.slot_index.to_string()])
        .args(["--key-id", &hsm.key_id])
        .arg("--pin-file")
        .arg(&pin_file)
        .assert()
        .success();

    // Delete the identity
    ctx.icp()
        .args(["identity", "delete", "hsm-identity"])
        .assert()
        .success()
        .stderr(contains("Deleted identity `hsm-identity`"));

    // Verify it's gone
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("hsm-identity").not());
}

/// After unlocking a password-protected identity once, subsequent commands must succeed
/// even when the password file is empty (i.e. the session delegation is reused).
#[test]
fn pem_session_delegation_avoids_second_password_prompt() {
    let ctx = TestContext::new();

    let mut password_file = NamedTempFile::new().unwrap();
    password_file.write_all(b"test-password-xyz").unwrap();
    let password_path = password_file.into_temp_path();

    // Create a password-protected identity.
    ctx.icp()
        .args([
            "identity",
            "new",
            "pw-session-test",
            "--storage",
            "password",
        ])
        .arg("--storage-password-file")
        .arg(&password_path)
        .assert()
        .success();

    // First use: unlocks the PEM and creates a session delegation.
    ctx.icp()
        .arg("--identity-password-file")
        .arg(&password_path)
        .args(["identity", "principal", "--identity", "pw-session-test"])
        .assert()
        .success();

    // Second use: password file is empty — decryption would fail if attempted.
    // The session delegation must be used instead.
    let empty_file = NamedTempFile::new().unwrap();
    ctx.icp()
        .arg("--identity-password-file")
        .arg(empty_file.path())
        .args(["identity", "principal", "--identity", "pw-session-test"])
        .assert()
        .success();
}

/// `icp identity login --duration` explicitly creates a PEM session, allowing subsequent
/// commands to succeed without a password even when automatic session caching is disabled.
#[test]
fn pem_explicit_login_creates_session() {
    let ctx = TestContext::new();

    // Disable automatic session caching.
    ctx.icp()
        .args(["settings", "session-length", "disabled"])
        .assert()
        .success();

    let mut password_file = NamedTempFile::new().unwrap();
    password_file.write_all(b"test-password-xyz").unwrap();
    let password_path = password_file.into_temp_path();

    ctx.icp()
        .args([
            "identity",
            "new",
            "explicit-session-test",
            "--storage",
            "password",
        ])
        .arg("--storage-password-file")
        .arg(&password_path)
        .assert()
        .success();

    // Explicit login creates the session delegation.
    ctx.icp()
        .arg("--identity-password-file")
        .arg(&password_path)
        .args([
            "identity",
            "login",
            "explicit-session-test",
            "--duration",
            "10m",
        ])
        .assert()
        .success();

    // Session is now cached; subsequent commands succeed without a password.
    let empty_file = NamedTempFile::new().unwrap();
    ctx.icp()
        .arg("--identity-password-file")
        .arg(empty_file.path())
        .args([
            "identity",
            "principal",
            "--identity",
            "explicit-session-test",
        ])
        .assert()
        .success();
}

/// When automatic session caching is disabled and `--duration` is omitted,
/// `icp identity login` must fail with a clear error for PEM identities.
#[test]
fn pem_login_requires_duration_when_sessions_disabled() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["settings", "session-length", "disabled"])
        .assert()
        .success();

    let mut password_file = NamedTempFile::new().unwrap();
    password_file.write_all(b"test-password-xyz").unwrap();
    let password_path = password_file.into_temp_path();

    ctx.icp()
        .args([
            "identity",
            "new",
            "no-duration-test",
            "--storage",
            "password",
        ])
        .arg("--storage-password-file")
        .arg(&password_path)
        .assert()
        .success();

    ctx.icp()
        .args(["identity", "login", "no-duration-test"])
        .assert()
        .failure()
        .stderr(contains("--duration"));
}
