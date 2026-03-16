use camino_tempfile::tempdir;
use icp::fs::{create_dir_all, write, write_string};

use crate::common::TestContext;

mod common;

/// Run `icp new` with a local template path, no VCS init, and the given project name.
/// Returns the path where the project was generated.
fn icp_new(ctx: &TestContext, template_path: &icp::prelude::Path, name: &str) {
    ctx.icp()
        .args([
            "new",
            "--path",
            template_path.as_str(),
            "--vcs",
            "none",
            name,
        ])
        .assert()
        .success();
}

#[test]
fn new_injects_gitkeep_when_template_does_not_have_one() {
    let ctx = TestContext::new();

    let template = tempdir().unwrap();
    write_string(&template.path().join("icp.yaml"), "").unwrap();

    icp_new(&ctx, template.path(), "my-project");

    assert!(
        ctx.home_path()
            .join("my-project/.icp/data/.gitkeep")
            .exists()
    );
}

#[test]
fn new_succeeds_when_template_already_has_gitkeep() {
    let ctx = TestContext::new();

    let template = tempdir().unwrap();
    write_string(&template.path().join("icp.yaml"), "").unwrap();
    create_dir_all(&template.path().join(".icp/data")).unwrap();
    write(&template.path().join(".icp/data/.gitkeep"), &[]).unwrap();

    icp_new(&ctx, template.path(), "my-project");

    assert!(
        ctx.home_path()
            .join("my-project/.icp/data/.gitkeep")
            .exists()
    );
}
