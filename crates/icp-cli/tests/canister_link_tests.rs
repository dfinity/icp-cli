use indoc::formatdoc;
use predicates::str::contains;

use crate::common::{ENVIRONMENT_RANDOM_PORT, NETWORK_RANDOM_PORT, TestContext};
use icp::{fs::write_string, prelude::*};

mod common;

/// A well-formed canister principal used as the link target. `link` never contacts a
/// network, so the canister does not need to actually exist.
const LINKED_ID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

fn write_manifest(project_dir: &Path) {
    let pm = formatdoc! {r#"
        canisters:
          - name: my-canister
            build:
              steps:
                - type: script
                  command: echo hi
          - name: other-canister
            build:
              steps:
                - type: script
                  command: echo hi

        {NETWORK_RANDOM_PORT}
        {ENVIRONMENT_RANDOM_PORT}
    "#};

    write_string(&project_dir.join("icp.yaml"), &pm).expect("failed to write project manifest");
}

fn mapping_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".icp")
        .join("cache")
        .join("mappings")
        .join("random-environment.ids.json")
}

#[tokio::test]
async fn canister_link_records_id() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    write_manifest(&project_dir);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "link",
            "my-canister",
            LINKED_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .success();

    let path = mapping_path(&project_dir);
    assert!(path.exists(), "ID mapping file should exist at {path}");

    let mapping = icp::fs::read_to_string(&path).expect("failed to read mapping file");
    assert!(
        mapping.contains(LINKED_ID),
        "mapping should contain the linked ID, got: {mapping}"
    );
}

#[tokio::test]
async fn canister_link_unknown_name_fails() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    write_manifest(&project_dir);

    ctx.icp()
        .current_dir(&project_dir)
        .args([
            "canister",
            "link",
            "not-a-canister",
            LINKED_ID,
            "--environment",
            "random-environment",
        ])
        .assert()
        .failure();
}

#[tokio::test]
async fn canister_link_duplicate_principal_fails() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    write_manifest(&project_dir);

    let link = |name: &str| {
        let mut cmd = ctx.icp();
        cmd.current_dir(&project_dir).args([
            "canister",
            "link",
            name,
            LINKED_ID,
            "--environment",
            "random-environment",
        ]);
        cmd
    };

    link("my-canister").assert().success();

    // The same principal cannot be linked to a second canister.
    link("other-canister")
        .assert()
        .failure()
        .stderr(contains("already linked to 'my-canister'"));
}

#[tokio::test]
async fn canister_link_existing_requires_force() {
    let ctx = TestContext::new();
    let project_dir = ctx.create_project_dir("icp");
    write_manifest(&project_dir);

    let link = |id: &str, force: bool| {
        let mut cmd = ctx.icp();
        cmd.current_dir(&project_dir).args([
            "canister",
            "link",
            "my-canister",
            id,
            "--environment",
            "random-environment",
        ]);
        if force {
            cmd.arg("--force");
        }
        cmd
    };

    link(LINKED_ID, false).assert().success();

    // A second link to a different ID must fail without --force.
    let other_id = "ryjl3-tyaaa-aaaaa-aaaba-cai";
    link(other_id, false)
        .assert()
        .failure()
        .stderr(contains("already registered"));

    // With --force it overwrites the recorded ID.
    link(other_id, true).assert().success();

    let mapping = icp::fs::read_to_string(&mapping_path(&project_dir)).expect("read mapping");
    assert!(
        mapping.contains(other_id) && !mapping.contains(LINKED_ID),
        "mapping should hold the forced ID only, got: {mapping}"
    );
}
