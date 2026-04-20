use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::TestContext;
use icp::fs::write_string;

mod common;

const PROJECT_MANIFEST: &str = r#"
canisters:
  - name: backend
    build:
      steps:
        - type: pre-built
          path: backend.wasm

networks:
  - name: local
    mode: managed

environments:
  - name: local
    network: local
  - name: staging
    network: local
"#;

fn setup_project(ctx: &TestContext) -> icp::prelude::PathBuf {
    let project_dir = ctx.create_project_dir("myproject");
    write_string(&project_dir.join("icp.yaml"), PROJECT_MANIFEST)
        .expect("failed to write project manifest");
    project_dir
}

#[tokio::test]
async fn canister_id_set_and_show() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    let canister_id = "rrkah-fqaaa-aaaaa-aaaaq-cai";

    // Set canister ID
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", canister_id])
        .assert()
        .success()
        .stdout(contains(canister_id));

    // Show canister ID
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend"])
        .assert()
        .success()
        .stdout(contains(canister_id));
}

#[tokio::test]
async fn canister_id_show_json() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    let canister_id = "rrkah-fqaaa-aaaaa-aaaaq-cai";

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", canister_id])
        .assert()
        .success();

    let output = ctx
        .icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("invalid JSON output");
    assert_eq!(json["canister"], "backend");
    assert_eq!(json["canister_id"], canister_id);
    assert_eq!(json["environment"], "local");
}

#[tokio::test]
async fn canister_id_set_rejects_duplicate_without_force() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    let first_id = "rrkah-fqaaa-aaaaa-aaaaq-cai";
    let second_id = "ryjl3-tyaaa-aaaaa-aaaba-cai";

    // First set succeeds
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", first_id])
        .assert()
        .success();

    // Second set without --force fails
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", second_id])
        .assert()
        .failure()
        .stderr(contains("already has ID").and(contains("--force")));

    // Original ID is preserved
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend"])
        .assert()
        .success()
        .stdout(contains(first_id));
}

#[tokio::test]
async fn canister_id_set_force_overwrites() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    let first_id = "rrkah-fqaaa-aaaaa-aaaaq-cai";
    let second_id = "ryjl3-tyaaa-aaaaa-aaaba-cai";

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", first_id])
        .assert()
        .success();

    // Overwrite with --force
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", second_id, "--force"])
        .assert()
        .success();

    // New ID is returned
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend"])
        .assert()
        .success()
        .stdout(contains(second_id));
}

#[tokio::test]
async fn canister_id_set_with_environment() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    let local_id = "rrkah-fqaaa-aaaaa-aaaaq-cai";
    let staging_id = "ryjl3-tyaaa-aaaaa-aaaba-cai";

    // Set different IDs for different environments
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "set", "backend", local_id])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister", "id", "set", "backend", staging_id, "-e", "staging",
        ])
        .assert()
        .success();

    // Verify each environment has its own ID
    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend"])
        .assert()
        .success()
        .stdout(contains(local_id));

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend", "-e", "staging"])
        .assert()
        .success()
        .stdout(contains(staging_id));
}

#[tokio::test]
async fn canister_id_show_not_set() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["canister", "id", "show", "backend"])
        .assert()
        .failure()
        .stderr(contains("could not find ID"));
}

#[tokio::test]
async fn canister_id_set_unknown_canister() {
    let ctx = TestContext::new();
    let project_dir = setup_project(&ctx);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "id",
            "set",
            "nonexistent",
            "rrkah-fqaaa-aaaaa-aaaaq-cai",
        ])
        .assert()
        .failure()
        .stderr(contains("not found in environment"));
}
