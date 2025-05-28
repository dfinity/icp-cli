use core::str;
use std::io::Write;

use common::TestEnv;
use ic_agent::export::Principal;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};
use tempfile::NamedTempFile;

mod common;

#[test]
fn identity_anonymous() {
    let env = TestEnv::new();
    env.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(eq("* anonymous").trim());
    env.icp()
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
    let env = TestEnv::new();
    env.icp()
        .args(["identity", "import", "alice", "--from-seed-file"])
        .arg(&path)
        .assert()
        .success();
    env.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("alice"))
        .stdout(contains("anonymous"));
    env.icp()
        .args(["identity", "default", "alice"])
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[test]
fn identity_import_pem() {
    let env = TestEnv::new();
    env.icp()
        .args([
            "identity",
            "import",
            "alice",
            "--from-pem",
            "tests/decrypted.pem",
        ])
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"swordfish").unwrap();
    let path = file.into_temp_path();
    env.icp()
        .args([
            "identity",
            "import",
            "bob",
            "--from-pem",
            "tests/encrypted.pem",
            "--decryption-password-from-file",
        ])
        .arg(&path)
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "bob"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[test]
fn identity_create() {
    let env = TestEnv::new();
    let new_out = env
        .icp()
        .args(["identity", "new", "alice"])
        .assert()
        .success();
    let seed = str::from_utf8(&new_out.get_output().stdout)
        .unwrap()
        .strip_prefix("Your seed phrase: ")
        .unwrap();
    env.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(contains("alice"))
        .stdout(contains("anonymous"));
    let principal1_out = env
        .icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success();
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(seed.trim().as_bytes()).unwrap();
    let path = file.into_temp_path();
    env.icp()
        .args(["identity", "import", "bob", "--from-seed-file"])
        .arg(&path)
        .assert()
        .success();
    let principal2_out = env
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
