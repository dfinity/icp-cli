use indoc::formatdoc;
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext, clients};
use icp::{fs::write_string, prelude::*};

mod common;

/// Deploying a project that declares a dependency should deploy the whole
/// dependency and wire canister IDs per project scope:
/// - the app's canister sees the exposed dependency canister under its alias,
/// - the dependency's canisters keep their standalone view (own names only).
#[tokio::test]
async fn deploy_with_dependency_injects_namespaced_ids() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");

    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // A self-contained vendored dependency project with two canisters.
    let dep_dir = project_dir.join("vendor/openemail");
    std::fs::create_dir_all(&dep_dir).expect("failed to create dependency dir");
    let dep_manifest = formatdoc! {r#"
        canisters:
          - name: backend
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
          - name: frontend
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
    "#};
    write_string(&dep_dir.join("icp.yaml"), &dep_manifest)
        .expect("failed to write dependency manifest");

    // The app: one canister plus a dependency exposing only `openemail:backend`.
    let pm = formatdoc! {r#"
        canisters:
          - name: backend
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"

        dependencies:
          - name: openemail
            path: ./vendor/openemail
            canisters: [backend]

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(200 * TRILLION);

    // Deploy the app and the whole dependency. The dependency canisters are keyed
    // by their path relative to the project root.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stdout(contains("vendor/openemail:backend").and(contains("vendor/openemail:frontend")));

    // The app's `backend` sees the exposed dependency canister under its alias.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "backend",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(contains("PUBLIC_CANISTER_ID:openemail:backend"));

    // The dependency's own `backend` keeps its standalone view: it sees `backend`
    // (itself) and `frontend`, but not the parent's `openemail:` alias.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "settings",
            "show",
            "vendor/openemail:backend",
            "--environment",
            "random-environment",
        ])
        .assert()
        .success()
        .stdout(
            contains("PUBLIC_CANISTER_ID:backend")
                .and(contains("PUBLIC_CANISTER_ID:frontend"))
                .and(contains("PUBLIC_CANISTER_ID:openemail:backend").not()),
        );
}
