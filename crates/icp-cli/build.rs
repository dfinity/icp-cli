use std::process::{Command, Stdio};

mod artifacts;

/// Gets a git tag with the least number of revs between HEAD of current branch and the tag,
/// and combines is with SHA of the HEAD commit. Example of expected output: `0.12.0-beta.1-b9ace030`
fn get_git_version() -> Result<String, std::io::Error> {
    let mut latest_tag = String::from("0");
    let mut latest_distance = u128::MAX;
    let tags = Command::new("git")
        .arg("tag")
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output()?
        .stdout;

    for tag in String::from_utf8_lossy(&tags).split_whitespace() {
        let output = Command::new("git")
            .arg("rev-list")
            .arg("--count")
            .arg(format!("{tag}..HEAD"))
            .arg(tag)
            .stdout(Stdio::piped())
            .spawn()?
            .wait_with_output()?
            .stdout;

        if let Some(count) = String::from_utf8_lossy(&output)
            .split_whitespace()
            .next()
            .and_then(|v| v.parse::<u128>().ok())
            && count < latest_distance
        {
            latest_tag = String::from(tag);
            latest_distance = count;
        }
    }

    let head_commit_sha = Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()?
        .stdout;
    let head_commit_sha = String::from_utf8_lossy(&head_commit_sha);
    let is_dirty = !Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .output()?
        .stdout
        .is_empty();

    Ok(format!(
        "{latest_tag}+rev{count}.{head_status}{head_commit_sha}",
        count = latest_distance,
        head_status = if is_dirty { "dirty-" } else { "" }
    ))
}

/// Use a version based on environment variable,
/// or the latest git tag plus sha of current git HEAD at time of build,
/// or let the cargo.toml version.
fn define_icp_cli_version() {
    // option_env! automatically detect changes and trigger rebuilds.
    // "cargo:rerun-if-env-changed" is not needed here.
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-env-changed
    if let Some(v) = option_env!("ICP_CLI_VERSION") {
        // If the version is passed in the environment, use that.
        // Used by the release process in .github/workflows/publish.yml
        println!("cargo:rustc-env=CARGO_PKG_VERSION={v}");
    } else if let Ok(git) = get_git_version() {
        // If the version isn't passed in the environment, use the git describe version.
        // Used when building from source.
        println!("cargo:rustc-env=CARGO_PKG_VERSION={git}");
    } else {
        // Nothing to do here, as there is no GIT. We keep the CARGO_PKG_VERSION.
    }
}

fn define_git_sha() {
    let git_sha = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .expect("Failed to run git rev-parse")
        .stdout;
    println!(
        "cargo:rustc-env=GIT_SHA={}",
        String::from_utf8_lossy(&git_sha)
    );
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=artifacts/mod.rs");
    println!("cargo:rerun-if-changed=artifacts/source.json");

    define_icp_cli_version();
    define_git_sha();
    artifacts::bundle_artifacts();
}
