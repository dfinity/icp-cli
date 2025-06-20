use core::str;
use std::io::Write;

use camino_tempfile::NamedUtf8TempFile;
use common::TestEnv;
use ic_agent::export::Principal;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

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
    let mut file = NamedUtf8TempFile::new().unwrap();
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
    // from plaintext sec1
    let env = TestEnv::new();
    env.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(env.make_asset("decrypted.pem"))
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    env.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(env.make_asset("missing_params.pem"))
        .assert()
        .failure()
        .stderr(contains("missing field `parameters`"));
    env.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(env.make_asset("unsupported_curve.pem"))
        .assert()
        .failure()
        .stderr(contains("unsupported algorithm"));

    env.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(env.make_asset("separate_params.pem"))
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "bob"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    env.icp()
        .args(["identity", "import", "carol", "--from-pem"])
        .arg(env.make_asset("missing_params.pem"))
        .args(["--assert-key-type", "secp256k1"])
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "carol"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    // from encrypted pkcs8
    let mut file = NamedUtf8TempFile::new().unwrap();
    file.write_all(b"swordfish").unwrap();
    let path = file.into_temp_path();
    env.icp()
        .args(["identity", "import", "chlöe", "--from-pem"])
        .arg(env.make_asset("encrypted.pem"))
        .arg("--decryption-password-from-file")
        .arg(&path)
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "chlöe"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    // from plaintext pkcs8
    env.icp()
        .args(["identity", "import", "d'artagnan", "--from-pem"])
        .arg(env.make_asset("pkcs8.pem"))
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal", "--identity", "d'artagnan"])
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
    let mut file = NamedUtf8TempFile::new().unwrap();
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

#[test]
fn identity_use() {
    let env = TestEnv::new();
    env.icp()
        .args(["identity", "import", "alice", "--from-pem"])
        .arg(env.make_asset("decrypted.pem"))
        .assert()
        .success();
    env.icp()
        .args(["identity", "default", "alice"])
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
    env.icp()
        .args(["identity", "principal", "--identity", "anonymous"])
        .assert()
        .success()
        .stdout(eq("2vxsx-fae").trim());
    env.icp()
        .args(["identity", "default", "anonymous"])
        .assert()
        .success();
    env.icp()
        .args(["identity", "principal"])
        .assert()
        .success()
        .stdout(eq("2vxsx-fae").trim());
    env.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[test]
fn identity_list() {
    let env = TestEnv::new();
    env.icp()
        .args(["identity", "list"])
        .assert()
        .success()
        .stdout(eq("* anonymous\n"));
    env.icp()
        .args(["identity", "list", "--format", "json"])
        .assert()
        .success()
        .stdout(eq("{\"v\":1,\"identities\":{\"anonymous\":{\"kind\":\"anonymous\"}}}\n"));
    
}
