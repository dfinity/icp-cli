use indoc::{formatdoc, indoc};
use k256::sha2::{Digest, Sha256};
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::{TestContext, spawn_test_server};
use icp::fs::write_string;

mod common;

const RECIPE_TEMPLATE: &str = indoc! {"
    build:
      steps:
        - type: script
          command: sh -c 'echo \"test\" > \"$ICP_WASM_OUTPUT_PATH\"'
"};
const RECIPE_TEMPLATE_CHECKSUM: &str =
    "475752eb684a14dcddcb0884c275285b03b53d5519a51925d3e05eb01a494d68";

#[test]
fn recipe_remote_url_without_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", RECIPE_TEMPLATE.as_bytes());
    let addr = server.addr();

    // Project manifest
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
    "};

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

    // Calculate checksum
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(RECIPE_TEMPLATE.as_bytes());
        h.finalize()
    });

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", RECIPE_TEMPLATE.as_bytes());
    let addr = server.addr();

    // Project manifest with invalid checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: invalid
    "};

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

    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(RECIPE_TEMPLATE.as_bytes());
        h.finalize()
    });

    // A random wrong but valid checksum
    let expected = "508f107eba1885bd165f4ef75c1b8cf914cf9eda6365f93214bb0cc39bd07ddd";

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", RECIPE_TEMPLATE.as_bytes());
    let addr = server.addr();

    // Project manifest with wrong checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: {expected}
    "};

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

    // Spawn HTTP server with the recipe template
    let server = spawn_test_server("GET", "/recipe.hbs", RECIPE_TEMPLATE.as_bytes());
    let addr = server.addr();

    // Project manifest with valid checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: http://{addr}/recipe.hbs
            sha256: {RECIPE_TEMPLATE_CHECKSUM}
    "};

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
fn recipe_local_file_without_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("recipe.hbs"), // path
        RECIPE_TEMPLATE,                 // contents
    )
    .expect("failed to write recipe template");

    // Project manifest without checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: file://./recipe.hbs
    "};

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
fn recipe_local_file_invalid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("recipe.hbs"), // path
        RECIPE_TEMPLATE,                 // contents
    )
    .expect("failed to write recipe template");

    // Project manifest with invalid checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: file://./recipe.hbs
            sha256: invalid
    "};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build - should fail due to invalid checksum
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(
            contains("checksum mismatch")
                .and(contains("expected invalid"))
                .and(contains(format!("actual {RECIPE_TEMPLATE_CHECKSUM}"))),
        );
}

#[test]
fn recipe_local_file_wrong_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("recipe.hbs"), // path
        RECIPE_TEMPLATE,                 // contents
    )
    .expect("failed to write recipe template");

    // A random wrong but valid checksum
    let expected = "508f107eba1885bd165f4ef75c1b8cf914cf9eda6365f93214bb0cc39bd07ddd";

    // Project manifest with wrong checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: file://./recipe.hbs
            sha256: {expected}
    "};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build - should fail due to wrong checksum
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .failure()
        .stderr(
            contains("checksum mismatch")
                .and(contains(format!("expected {expected}")))
                .and(contains(format!("actual {RECIPE_TEMPLATE_CHECKSUM}"))),
        );
}

#[test]
fn recipe_local_file_valid_checksum() {
    let ctx = TestContext::new();

    // Setup project
    let project_dir = ctx.create_project_dir("icp");

    write_string(
        &project_dir.join("recipe.hbs"), // path
        RECIPE_TEMPLATE,                 // contents
    )
    .expect("failed to write recipe template");

    // Project manifest with valid checksum
    let pm = formatdoc! {"
        canister:
          name: my-canister
          recipe:
            type: file://./recipe.hbs
            sha256: {RECIPE_TEMPLATE_CHECKSUM}
    "};

    write_string(
        &project_dir.join("icp.yaml"), // path
        &pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Invoke build - should succeed with valid checksum
    ctx.icp()
        .current_dir(project_dir)
        .args(["build"])
        .assert()
        .success();
}

#[test]
fn recipe_builtin_ignores_checksum() {
    let ctx = TestContext::new();

    for recipe_type in ["assets", "motoko", "rust"] {
        // Setup project
        let project_dir = ctx.create_project_dir("icp");

        let pm = formatdoc! {"
            canister:
            name: my-canister
            recipe:
                type: {recipe_type}
                sha256: invalid_checksum_should_be_ignored
        "};

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
            .append_context("test-case", recipe_type)
            .success();
    }
}
