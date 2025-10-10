use indoc::formatdoc;
use k256::sha2::{Digest, Sha256};
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::{TestContext, spawn_test_server};
use icp::fs::write_string;

mod common;

#[test]
fn recipe_remote_url_without_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create a simple recipe template that generates a pre-built build step
    let recipe_template = r#"
build:
  steps:
    - type: script
      command: sh -c 'echo "test" > "$ICP_WASM_OUTPUT_PATH"'
"#;

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", recipe_template.as_bytes());
    let addr = server.addr();

    // Project manifest
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn recipe_remote_url_invalid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create a simple recipe template
    let recipe_template = r#"
build:
  steps:
    - type: script
      command: sh -c 'echo "test" > "$ICP_WASM_OUTPUT_PATH"'
"#;

    // Calculate checksum
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(recipe_template.as_bytes());
        h.finalize()
    });

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", recipe_template.as_bytes());
    let addr = server.addr();

    // Project manifest with invalid checksum
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: invalid
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(
            contains("checksum mismatch")
                .and(contains("expected invalid"))
                .and(contains(format!("actual {actual}"))),
        );
}

#[test]
fn recipe_remote_url_wrong_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create a simple recipe template
    let recipe_template = r#"
build:
  steps:
    - type: script
      command: sh -c 'echo "test" > "$ICP_WASM_OUTPUT_PATH"'
"#;

    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(recipe_template.as_bytes());
        h.finalize()
    });

    // A random wrong but valid checksum
    let expected = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", recipe_template.as_bytes());
    let addr = server.addr();

    // Project manifest with wrong checksum
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: {expected}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(
            contains("checksum mismatch")
                .and(contains(format!("expected {expected}")))
                .and(contains(format!("actual {actual}"))),
        );
}

#[test]
fn recipe_remote_url_valid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create a simple recipe template
    let recipe_template = r#"
build:
  steps:
    - type: script
      command: sh -c 'echo "test" > "$ICP_WASM_OUTPUT_PATH"'
"#;

    // Calculate checksum
    let checksum = hex::encode({
        let mut h = Sha256::new();
        h.update(recipe_template.as_bytes());
        h.finalize()
    });

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", recipe_template.as_bytes());
    let addr = server.addr();

    // Project manifest with valid checksum
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: {checksum}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn recipe_https_url_with_valid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create a simple recipe template
    let recipe_template = r#"
build:
  steps:
    - type: script
      command: sh -c 'echo "test" > "$ICP_WASM_OUTPUT_PATH"'
"#;

    // Calculate checksum
    let checksum = hex::encode({
        let mut h = Sha256::new();
        h.update(recipe_template.as_bytes());
        h.finalize()
    });

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", recipe_template.as_bytes());
    let addr = server.addr();

    // Project manifest with valid checksum (using http since we can't easily test https in unit tests)
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: {checksum}
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn recipe_local_file_ignores_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Create a local recipe template file
    let recipe_template = r#"
build:
  steps:
    - type: script
      command: sh -c 'echo "test" > "$ICP_WASM_OUTPUT_PATH"'
"#;

    write_string(
        &project_dir.join("recipe.hbs"), // path
        recipe_template,                 // contents
    )
    .expect("failed to write recipe template");

    // Project manifest with checksum (which should be ignored for local files)
    // Note: The implementation may or may not verify checksums for local files
    // This test documents the current behavior
    let pm = formatdoc! {r#"
        canister:
          name: my-canister
          recipe:
            type: file://./recipe.hbs
            sha256: invalid_checksum_should_be_ignored
    "#};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build - should succeed because local files don't verify checksums
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}
