use core::str;
use std::io::Write;

use camino_tempfile::NamedUtf8TempFile as NamedTempFile;
use common::TestContext;
use ic_agent::export::Principal;
use predicates::{
    ord::eq,
    str::{PredicateStrExt, contains},
};

use crate::common::clients;

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
        .arg(ctx.make_asset("decrypted.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "alice"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("missing_params.pem"))
        .assert()
        .failure()
        .stderr(contains("missing field `parameters`"));
    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("unsupported_curve.pem"))
        .assert()
        .failure()
        .stderr(contains("unsupported algorithm"));

    ctx.icp()
        .args(["identity", "import", "bob", "--from-pem"])
        .arg(ctx.make_asset("separate_params.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "bob"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());

    ctx.icp()
        .args(["identity", "import", "carol", "--from-pem"])
        .arg(ctx.make_asset("missing_params.pem"))
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
        .arg(ctx.make_asset("encrypted.pem"))
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
        .arg(ctx.make_asset("pkcs8.pem"))
        .assert()
        .success();
    ctx.icp()
        .args(["identity", "principal", "--identity", "d'artagnan"])
        .assert()
        .success()
        .stdout(eq("5upke-tazvi-6ufqc-i3v6r-j4gpu-dpwti-obhal-yb5xj-ue32x-ktkql-rqe").trim());
}

#[test]
fn identity_create() {
    let ctx = TestContext::new();

    let new_out = ctx
        .icp()
        .args(["identity", "new", "alice"])
        .assert()
        .success();

    let alice_principal = clients::icp(&ctx).get_principal("alice");
    let anonymous_principal = clients::icp(&ctx).get_principal("anonymous");

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
        .arg(ctx.make_asset("decrypted.pem"))
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
