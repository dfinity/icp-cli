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

        # A member must declare every environment the workspace targets;
        # the network binding is ignored (the root supplies it).
        environments:
          - name: random-environment
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

/// Running `icp deploy` from *inside* a vendored member resolves up to the
/// workspace root and deploys only that member's canisters, into the root's
/// environment and store (single source-of-truth ids). The app's own canister
/// is not touched.
#[tokio::test]
async fn deploy_from_member_scopes_to_member_and_uses_root_store() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // A self-contained vendored dependency with two canisters.
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

        # A member must declare every environment the workspace targets;
        # the network binding is ignored (the root supplies it).
        environments:
          - name: random-environment
    "#};
    write_string(&dep_dir.join("icp.yaml"), &dep_manifest)
        .expect("failed to write dependency manifest");

    // The app: its own canister plus the dependency. The network/environment
    // live only in the app (the workspace root).
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

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    let _g = ctx.start_network_in(&project_dir, "random-network").await;
    ctx.ping_until_healthy(&project_dir, "random-network");

    clients::icp(&ctx, &project_dir, Some("random-environment".to_string()))
        .mint_cycles(200 * TRILLION);

    // Deploy from INSIDE the member. Resolution climbs to the app root; only the
    // member's canisters are deployed, and the run announces the resolved root.
    ctx.icp()
        .current_dir(&dep_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success()
        .stdout(contains("vendor/openemail:backend").and(contains("vendor/openemail:frontend")))
        .stderr(contains("resolved workspace root"));

    // The member's ids were written to the *root* store: they are queryable from
    // the app root.
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "--environment",
            "random-environment",
            "vendor/openemail:backend",
            "--id-only",
        ])
        .assert()
        .success();

    // The app's own canister was NOT deployed by the member-scoped run (no id in
    // the store yet).
    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "status",
            "--environment",
            "random-environment",
            "backend",
            "--id-only",
        ])
        .assert()
        .failure();
}

/// Deploying to an environment a vendored member does not declare fails fast
/// with a clear error (strict rule) — before any network is contacted.
#[tokio::test]
async fn deploy_to_env_missing_from_member_fails() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    // The dependency does NOT declare `random-environment`.
    let dep_dir = project_dir.join("vendor/openemail");
    std::fs::create_dir_all(&dep_dir).expect("failed to create dependency dir");
    let dep_manifest = formatdoc! {r#"
        canisters:
          - name: backend
            build:
              steps:
                - type: script
                  command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
    "#};
    write_string(&dep_dir.join("icp.yaml"), &dep_manifest)
        .expect("failed to write dependency manifest");

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

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};
    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");

    // No network started: the strict check rejects the deploy before any network
    // interaction.
    ctx.icp()
        .current_dir(&project_dir)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .failure()
        .stderr(contains("random-environment").and(contains("vendor/openemail")));
}

/// The "umbrella" layout: two independent sub-projects (`service-a`, `service-b`)
/// each depend on the same sibling `openemail` via `../openemail`, and the app
/// depends on both services. Because both edges resolve to the same directory,
/// openemail must be deployed exactly once and shared by both services.
#[tokio::test]
async fn deploy_with_shared_dependency_dedups_to_one_instance() {
    let ctx = TestContext::new();
    let app = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");

    let canister = |name: &str| {
        formatdoc! {r#"
            canisters:
              - name: {name}
                build:
                  steps:
                    - type: script
                      command: cp '{wasm}' "$ICP_WASM_OUTPUT_PATH"
        "#}
    };

    // Every member must declare the environment the workspace targets.
    let random_env = "\nenvironments:\n  - name: random-environment\n";

    // umbrella/openemail — the shared service.
    let openemail = app.join("umbrella/openemail");
    std::fs::create_dir_all(&openemail).expect("failed to create openemail dir");
    write_string(
        &openemail.join("icp.yaml"),
        &format!("{}{random_env}", canister("backend")),
    )
    .expect("failed to write openemail manifest");

    // umbrella/service-a and umbrella/service-b each depend on ../openemail.
    for svc in ["service-a", "service-b"] {
        let dir = app.join(format!("umbrella/{svc}"));
        std::fs::create_dir_all(&dir).expect("failed to create service dir");
        let manifest = formatdoc! {r#"
            {service}
            dependencies:
              - name: openemail
                path: ../openemail
                canisters: [backend]
            {random_env}
        "#, service = canister("service")};
        write_string(&dir.join("icp.yaml"), &manifest).expect("failed to write service manifest");
    }

    // The app depends on both services.
    let pm = formatdoc! {r#"
        {frontend}
        dependencies:
          - name: service-a
            path: ./umbrella/service-a
            canisters: [service]
          - name: service-b
            path: ./umbrella/service-b
            canisters: [service]

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#, frontend = canister("frontend")};
    write_string(&app.join("icp.yaml"), &pm).expect("failed to write app manifest");

    let _g = ctx.start_network_in(&app, "random-network").await;
    ctx.ping_until_healthy(&app, "random-network");

    clients::icp(&ctx, &app, Some("random-environment".to_string())).mint_cycles(500 * TRILLION);

    // Deploy succeeds: the two edges to `umbrella/openemail` collapse to one
    // instance. (Without de-dup, importing the same store key twice would error.)
    ctx.icp()
        .current_dir(&app)
        .args(["deploy", "--environment", "random-environment"])
        .assert()
        .success();

    // Capture the single shared openemail canister id.
    let assert = ctx
        .icp()
        .current_dir(&app)
        .args([
            "canister",
            "status",
            "--environment",
            "random-environment",
            "umbrella/openemail:backend",
            "--id-only",
        ])
        .assert()
        .success();
    let openemail_id = String::from_utf8(assert.get_output().stdout.clone())
        .expect("canister id should be valid utf-8")
        .trim()
        .to_string();
    assert!(
        !openemail_id.is_empty(),
        "expected a shared openemail canister id"
    );

    // Both services' `openemail:backend` binding resolves to the SAME instance.
    for svc in ["umbrella/service-a:service", "umbrella/service-b:service"] {
        ctx.icp()
            .current_dir(&app)
            .args([
                "canister",
                "settings",
                "show",
                svc,
                "--environment",
                "random-environment",
            ])
            .assert()
            .success()
            .stdout(
                contains("PUBLIC_CANISTER_ID:openemail:backend")
                    .and(contains(openemail_id.clone())),
            );
    }
}
