use predicates::{ord::eq, prelude::*};

mod common;
use common::TestContext;

#[test]
fn settings_autocontainerize_default() {
    let ctx = TestContext::new();

    // Default value should be false
    ctx.icp()
        .args(["settings", "autocontainerize"])
        .assert()
        .success()
        .stdout(eq("false").trim());
}

#[test]
fn settings_autocontainerize_set_true() {
    let ctx = TestContext::new();

    // Set to true
    ctx.icp()
        .args(["settings", "autocontainerize", "true"])
        .assert()
        .success()
        .stdout(eq("Set autocontainerize to true").trim());

    // Verify it's now true
    ctx.icp()
        .args(["settings", "autocontainerize"])
        .assert()
        .success()
        .stdout(eq("true").trim());
}

#[test]
fn settings_autocontainerize_set_false() {
    let ctx = TestContext::new();

    // Set to true first
    ctx.icp()
        .args(["settings", "autocontainerize", "true"])
        .assert()
        .success();

    // Set back to false
    ctx.icp()
        .args(["settings", "autocontainerize", "false"])
        .assert()
        .success()
        .stdout(eq("Set autocontainerize to false").trim());

    // Verify it's now false
    ctx.icp()
        .args(["settings", "autocontainerize"])
        .assert()
        .success()
        .stdout(eq("false").trim());
}

#[test]
fn settings_autocontainerize_persists() {
    let ctx = TestContext::new();

    // Set to true
    ctx.icp()
        .args(["settings", "autocontainerize", "true"])
        .assert()
        .success();

    // Verify it persists across multiple reads
    ctx.icp()
        .args(["settings", "autocontainerize"])
        .assert()
        .success()
        .stdout(eq("true").trim());

    ctx.icp()
        .args(["settings", "autocontainerize"])
        .assert()
        .success()
        .stdout(eq("true").trim());
}
