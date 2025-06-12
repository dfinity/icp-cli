use icp_adapter::script::{CommandField, ScriptAdapter};
use icp_canister::model::{Adapter, Build, CanisterManifest};
use icp_project::{
    model::{LoadProjectManifestError, ProjectManifest},
    structure::ProjectDirectoryStructure,
};
mod common;
use common::TestEnv;

#[test]
fn empty_project() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Write project-manifest
    std::fs::write(
        project_dir.join("icp.yaml"), // path
        "",                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds).expect("failed to load project manifest");

    // Verify no canisters were found
    assert!(pm.canisters.is_empty());
}

#[test]
fn single_canister_project() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Write project-manifest
    let pm = r#"
    canister:
      name: canister-1
      build:
        adapter:
          type: script
          command: echo test
    "#;

    std::fs::write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds).expect("failed to load project manifest");

    // Verify canister was loaded
    let canisters = vec![(
        project_dir.to_owned(),
        CanisterManifest {
            name: "canister-1".into(),
            build: Build {
                adapter: Adapter::Script(ScriptAdapter {
                    command: CommandField::Command("echo test".into()),
                }),
            },
        },
    )];

    assert_eq!(pm.canisters, canisters);
}

#[test]
fn multi_canister_project() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Create canister directory
    std::fs::create_dir(project_dir.join("canister-1"))
        .expect("failed to create canister directory");

    // Write canister-manifest
    let cm = r#"
    name: canister-1
    build:
      adapter:
        type: script
        command: echo test
    "#;

    std::fs::write(
        project_dir.join("canister-1/canister.yaml"), // path
        cm,                                           // contents
    )
    .expect("failed to write canister manifest");

    // Write project-manifest
    let pm = r#"
    canisters:
      - canister-1
    "#;

    std::fs::write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds).expect("failed to load project manifest");

    // Verify canister was loaded
    let canisters = vec![(
        project_dir.join("canister-1"),
        CanisterManifest {
            name: "canister-1".into(),
            build: Build {
                adapter: Adapter::Script(ScriptAdapter {
                    command: CommandField::Command("echo test".into()),
                }),
            },
        },
    )];

    assert_eq!(pm.canisters, canisters);
}

#[test]
fn invalid_project_manifest() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Write project-manifest
    std::fs::write(
        project_dir.join("icp.yaml"), // path
        "invalid-content",            // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds);

    // Assert failure
    assert!(matches!(pm, Err(LoadProjectManifestError::Parse { .. })));
}

#[test]
fn invalid_canister_manifest() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Create canister directory
    std::fs::create_dir(project_dir.join("canister-1"))
        .expect("failed to create canister directory");

    // Write canister-manifest
    std::fs::write(
        project_dir.join("canister-1/canister.yaml"), // path
        "invalid-content",                            // contents
    )
    .expect("failed to write canister manifest");

    // Write project-manifest
    let pm = r#"
    canisters:
      - canister-1
    "#;

    std::fs::write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds);

    // Assert failure
    assert!(matches!(
        pm,
        Err(LoadProjectManifestError::CanisterLoad { .. })
    ));
}

#[test]
fn glob_path_non_canister() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Create canister directory
    std::fs::create_dir_all(project_dir.join("canisters/canister-1"))
        .expect("failed to create canister directory");

    // Skip writing canister-manifest
    //

    // Write project-manifest
    let pm = r#"
    canisters:
      - canisters/*
    "#;

    std::fs::write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds).expect("failed to load project manifest");

    // Verify no canisters were found
    assert!(pm.canisters.is_empty());
}

#[test]
fn explicit_path_non_canister() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Create canister directory
    std::fs::create_dir(project_dir.join("canister-1"))
        .expect("failed to create canister directory");

    // Skip writing canister-manifest
    //

    // Write project-manifest
    let pm = r#"
    canisters:
      - canister-1
    "#;

    std::fs::write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds);

    // Assert failure
    assert!(matches!(
        pm,
        Err(LoadProjectManifestError::NoManifest { .. })
    ));
}

#[test]
fn invalid_glob_pattern() {
    // Setup
    let env = TestEnv::new();
    let project_dir = env.home_path();

    // Write project-manifest
    let pm = r#"
    canisters:
      - canisters/***
    "#;

    std::fs::write(
        project_dir.join("icp.yaml"), // path
        pm,                           // contents
    )
    .expect("failed to write project manifest");

    // Load Project
    let pds = ProjectDirectoryStructure::new(project_dir);
    let pm = ProjectManifest::load(&pds);

    // Assert failure
    assert!(matches!(
        pm,
        Err(LoadProjectManifestError::GlobPattern { .. })
    ));
}
