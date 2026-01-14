use std::process::Command;

mod artifacts;

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

    define_git_sha();
    artifacts::bundle_artifacts();
}
