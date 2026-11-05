//! `--untrusted` holds a project to what one from elsewhere may do: see
//! `icp_project::untrusted`. Each test here sets up a tree the CLI would act
//! on as the user's own, and shows the flag refusing it, naming what it
//! refused. The one tree the flag accepts is the shape a bundle has.

use indoc::indoc;
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::TestContext;
use icp_project::fs::write_string;

mod common;

/// A project directory holding a module of its own, and beside it a file the
/// project must not reach.
fn project(ctx: &TestContext, manifest: &str) -> camino::Utf8PathBuf {
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    std::fs::copy(&wasm, project_dir.join("backend.wasm")).expect("copy wasm");
    std::fs::write(project_dir.parent().unwrap().join("outside.bin"), b"secret")
        .expect("write outside");
    write_string(&project_dir.join("icp.yaml"), manifest).expect("write manifest");
    project_dir
}

#[test]
fn a_bundle_shaped_project_builds_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                build:
                  steps:
                    - type: pre-built
                      path: backend.wasm
        "#},
    );

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .success();
}

#[test]
fn a_script_step_is_refused_as_untrusted_but_not_otherwise() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                build:
                  steps:
                    - type: script
                      command: cp backend.wasm "$ICP_WASM_OUTPUT_PATH"
        "#},
    );

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(
            contains("more than an untrusted project may")
                .and(contains("canister 'backend' has a script build step")),
        );
}

#[test]
fn a_recipe_is_not_fetched_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                recipe:
                  type: file://recipe.hbs
        "#},
    );
    std::fs::write(
        project_dir.join("recipe.hbs"),
        "build:\n  steps:\n    - type: pre-built\n      path: backend.wasm\n",
    )
    .expect("write recipe");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains("recipe 'file://recipe.hbs' is not fetched"));
}

#[test]
fn a_path_outside_the_project_is_refused_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                init_args:
                  path: ../outside.bin
                  format: bin
                build:
                  steps:
                    - type: pre-built
                      path: backend.wasm
        "#},
    );

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains("outside.bin").and(contains("is outside")));
}

#[cfg(unix)]
#[test]
fn a_symlink_under_the_project_is_refused_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                init_args:
                  path: args.bin
                  format: bin
                build:
                  steps:
                    - type: pre-built
                      path: backend.wasm
        "#},
    );
    std::os::unix::fs::symlink(
        project_dir.parent().unwrap().join("outside.bin"),
        project_dir.join("args.bin"),
    )
    .expect("symlink");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(
            contains("args.bin")
                .and(contains("passes through"))
                .and(contains("symlink")),
        );
}

#[test]
fn untrusted_needs_a_project_to_hold_to() {
    let ctx = TestContext::new();
    let empty = ctx.home_path().join("nowhere");
    std::fs::create_dir_all(&empty).expect("mkdir");

    ctx.icp()
        .current_dir(&empty)
        .args(["identity", "list", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains(
            "failed to locate the project to treat as untrusted",
        ));
}

#[test]
fn the_flag_is_global() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                build:
                  steps:
                    - type: pre-built
                      path: backend.wasm
        "#},
    );

    ctx.icp()
        .current_dir(&project_dir)
        .args(["--untrusted", "build", "backend"])
        .assert()
        .success();
}
