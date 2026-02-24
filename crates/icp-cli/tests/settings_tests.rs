use predicates::{ord::eq, prelude::*};

mod common;
use common::TestContext;

// ---------------------------------------------------------------------------
// autocontainerize
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// telemetry
// ---------------------------------------------------------------------------

#[test]
fn settings_telemetry_default() {
    let ctx = TestContext::new();

    // Default value should be true
    ctx.icp()
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stdout(eq("true").trim());
}

#[test]
fn settings_telemetry_set_false() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["settings", "telemetry", "false"])
        .assert()
        .success()
        .stdout(eq("Set telemetry to false").trim());

    ctx.icp()
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stdout(eq("false").trim());
}

#[test]
fn settings_telemetry_set_true() {
    let ctx = TestContext::new();

    // Disable first so we have a non-default state to switch from.
    ctx.icp()
        .args(["settings", "telemetry", "false"])
        .assert()
        .success();

    ctx.icp()
        .args(["settings", "telemetry", "true"])
        .assert()
        .success()
        .stdout(eq("Set telemetry to true").trim());

    ctx.icp()
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stdout(eq("true").trim());
}

#[test]
fn settings_telemetry_persists() {
    let ctx = TestContext::new();

    ctx.icp()
        .args(["settings", "telemetry", "false"])
        .assert()
        .success();

    ctx.icp()
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stdout(eq("false").trim());

    ctx.icp()
        .args(["settings", "telemetry"])
        .assert()
        .success()
        .stdout(eq("false").trim());
}
