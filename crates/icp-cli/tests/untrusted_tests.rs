//! `--untrusted` holds a project to what one from elsewhere may read and
//! run: see `icp_project::untrusted`. Each test here sets up a tree the CLI
//! would act on as the user's own, and shows the flag refusing it, naming
//! what it refused.

use indoc::indoc;
use predicates::{prelude::PredicateBooleanExt, str::contains};

use crate::common::TestContext;
use icp_project::fs::write_string;

mod common;

/// A project directory holding a module of its own, and beside it a file
/// outside the project: `outside.bin`, and `outside.wasm`, a module.
fn project(ctx: &TestContext, manifest: &str) -> camino::Utf8PathBuf {
    let project_dir = ctx.create_project_dir("icp");
    let wasm = ctx.make_asset("example_icp_mo.wasm");
    let outside = project_dir.parent().unwrap();
    std::fs::copy(&wasm, project_dir.join("backend.wasm")).expect("copy wasm");
    std::fs::copy(&wasm, outside.join("outside.wasm")).expect("copy wasm outside");
    std::fs::write(outside.join("outside.bin"), b"secret").expect("write outside");
    write_string(&project_dir.join("icp.yaml"), manifest).expect("write manifest");
    project_dir
}

const PREBUILT: &str = indoc! {r#"
    canisters:
      - name: backend
        build:
          steps:
            - type: pre-built
              path: backend.wasm
"#};

const SCRIPT: &str = indoc! {r#"
    canisters:
      - name: backend
        build:
          steps:
            - type: script
              command: cp backend.wasm "$ICP_WASM_OUTPUT_PATH"
"#};

#[test]
fn a_bundle_shaped_project_builds_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(&ctx, PREBUILT);

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .success();
}

#[test]
fn a_script_step_is_refused_as_untrusted_but_not_otherwise() {
    let ctx = TestContext::new();
    let project_dir = project(&ctx, SCRIPT);

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
                .and(contains(
                    "canister 'backend' has a build step that is not a pre-built module",
                ))
                .and(contains("(script) cp backend.wasm")),
        );
}

/// A recipe is read and rendered like any other; it is the steps it renders
/// to that are held to the list.
#[test]
fn a_recipe_is_rendered_and_its_steps_held_to_the_list() {
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
    let recipe = project_dir.join("recipe.hbs");

    std::fs::write(
        &recipe,
        "build:\n  steps:\n    - type: pre-built\n      path: backend.wasm\n",
    )
    .expect("write recipe");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .success();

    std::fs::write(
        &recipe,
        "build:\n  steps:\n    - type: script\n      command: cp backend.wasm \"$ICP_WASM_OUTPUT_PATH\"\n",
    )
    .expect("write recipe");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains("not a pre-built module").and(contains("(script) cp backend.wasm")));
}

/// What the project reads is held to its root: an argument file outside it
/// is read for the user's own project and refused for an untrusted one.
#[test]
fn a_file_read_from_outside_the_project_is_refused_as_untrusted() {
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

/// A local recipe template is read through the same filesystem.
#[test]
fn a_recipe_template_outside_the_project_is_refused_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                recipe:
                  type: file://../recipe.hbs
        "#},
    );
    std::fs::write(
        project_dir.parent().unwrap().join("recipe.hbs"),
        "build:\n  steps:\n    - type: pre-built\n      path: backend.wasm\n",
    )
    .expect("write recipe");

    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains("recipe.hbs").and(contains("is outside")));
}

#[test]
fn a_module_outside_the_project_is_refused_as_untrusted() {
    let ctx = TestContext::new();
    let project_dir = project(
        &ctx,
        indoc! {r#"
            canisters:
              - name: backend
                build:
                  steps:
                    - type: pre-built
                      path: ../outside.wasm
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
            contains("names '")
                .and(contains("outside.wasm"))
                .and(contains("not a regular file in the project")),
        );
}

/// A symlink is refused wherever it leads, out of the project or within it:
/// as the module a step names, and as a file the project reads.
#[cfg(unix)]
#[test]
fn a_symlink_is_refused_as_untrusted_wherever_it_leads() {
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
                      path: linked.wasm
        "#},
    );
    let module = project_dir.join("linked.wasm");
    let args = project_dir.join("args.bin");
    let outside = project_dir.parent().unwrap().to_path_buf();
    std::fs::write(&args, b"args").expect("write args");

    for target in [
        outside.join("outside.wasm"),
        project_dir.join("backend.wasm"),
    ] {
        std::os::unix::fs::symlink(&target, &module).expect("symlink");
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
            .stderr(contains("linked.wasm").and(contains("not a regular file in the project")));
        std::fs::remove_file(&module).expect("remove link");
    }

    std::fs::copy(project_dir.join("backend.wasm"), &module).expect("copy module");
    std::fs::remove_file(&args).expect("remove args");
    std::os::unix::fs::symlink(outside.join("outside.bin"), &args).expect("symlink");
    ctx.icp()
        .current_dir(&project_dir)
        .args(["build", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains("args.bin").and(contains("is a symlink")));
}

/// The root may be spelled relatively: it is taken from the working directory.
#[test]
fn a_relative_project_root_is_taken_from_the_working_directory() {
    let ctx = TestContext::new();
    let project_dir = project(&ctx, PREBUILT);
    let parent = project_dir.parent().unwrap();

    ctx.icp()
        .current_dir(parent)
        .args(["build", "--untrusted", "--project-root-override", "icp"])
        .assert()
        .success();

    ctx.icp()
        .current_dir(parent)
        .env("ICP_PROJECT_ROOT", "icp")
        .args(["build", "--untrusted"])
        .assert()
        .success();
}

/// The flag is on the three commands that act on what a project says, and
/// each refuses before it reaches any network. It is on no other command.
#[test]
fn the_flag_is_on_build_deploy_and_sync_alone() {
    let ctx = TestContext::new();
    let project_dir = project(&ctx, SCRIPT);

    for command in ["build", "deploy", "sync"] {
        ctx.icp()
            .current_dir(&project_dir)
            .args([command, "--untrusted"])
            .assert()
            .failure()
            .stderr(contains("more than an untrusted project may"));
    }

    ctx.icp()
        .current_dir(&project_dir)
        .args(["identity", "list", "--untrusted"])
        .assert()
        .failure()
        .stderr(contains("unexpected argument '--untrusted'"));
}
