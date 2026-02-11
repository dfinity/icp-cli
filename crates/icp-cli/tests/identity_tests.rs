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
        .stdout(contains("Renamed identity `alice` to `bob`"));

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
        .stdout(contains("Deleted identity `alice`"));

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
        .stdout(contains("Identity \"hsm-identity\" linked to HSM"));

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
        .stdout(contains("Deleted identity `hsm-identity`"));

    // Verify it's gone
    ctx.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("hsm-identity").not());
}
