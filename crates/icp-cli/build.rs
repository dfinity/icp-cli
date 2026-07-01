// It's okay to use std::path::{Path, PathBuf} in build scripts.
#![allow(clippy::disallowed_types)]

use std::path::PathBuf;
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

fn define_test_network_launcher_version() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../network-launcher-version"
    );
    let raw = std::fs::read_to_string(path).expect("missing network-launcher-version file");
    let version = raw.trim().trim_start_matches('v');
    println!("cargo:rustc-env=TEST_NETWORK_LAUNCHER_VERSION={version}");
    println!("cargo:rerun-if-changed=../../network-launcher-version");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    let constants = format!(
        r#"pub(crate) const NETWORK_RANDOM_PORT: &str = "networks:
  - name: random-network
    mode: managed
    gateway:
      port: 0
    version: v{version}
";

pub(crate) const NETWORK_DOCKER: &str = "networks:
  - name: docker-network
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher:{version}
    port-mapping:
      - 0:4943
      - 0:4942
";

pub(crate) const NETWORK_DOCKER_ENGINE: &str = "networks:
  - name: docker-engine-network
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher:{version}-engine
    port-mapping:
      - 0:4943
      - 0:4942
";
"#
    );
    std::fs::write(format!("{out_dir}/network_constants.rs"), constants).unwrap();
}

/// Builds the `recover-cycles-canister` crate to wasm and embeds it via the
/// `RECOVER_CYCLES_WASM` env var. Unlike the downloaded artifacts in
/// `source.json`, this canister lives in-tree and is compiled here on every
/// build. It is required for cycle recovery during `icp canister delete`, so a
/// missing `wasm32-unknown-unknown` target is a hard failure.
fn build_recover_cycles_canister() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let crate_manifest = manifest_dir.join("recover-cycles-canister/Cargo.toml");
    let target_dir = out_dir.join("recover-cycles-target");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    println!("cargo:rerun-if-changed=recover-cycles-canister/src/lib.rs");
    println!("cargo:rerun-if-changed=recover-cycles-canister/Cargo.toml");
    println!("cargo:rerun-if-changed=recover-cycles-canister/Cargo.lock");

    let status = Command::new(&cargo)
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--locked",
            "--manifest-path",
            crate_manifest.to_str().unwrap(),
            "--target-dir",
            target_dir.to_str().unwrap(),
        ])
        .status()
        .expect("failed to spawn cargo build for recover-cycles-canister");
    assert!(
        status.success(),
        "cargo build --target wasm32-unknown-unknown failed for recover-cycles-canister \
         (run `rustup target add wasm32-unknown-unknown`)"
    );

    let wasm = target_dir.join("wasm32-unknown-unknown/release/recover_cycles_canister.wasm");
    println!("cargo:rustc-env=RECOVER_CYCLES_WASM={}", wasm.display());
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=artifacts/mod.rs");
    println!("cargo:rerun-if-changed=artifacts/source.json");

    if option_env!("GIT_SHA").is_none() {
        define_git_sha();
    }
    define_test_network_launcher_version();
    artifacts::bundle_artifacts();
    build_recover_cycles_canister();
}
