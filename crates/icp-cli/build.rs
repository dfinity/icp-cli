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
    let raw = std::fs::read_to_string("test-network-launcher-version")
        .expect("missing test-network-launcher-version file");
    let version = raw.trim().trim_start_matches('v');
    println!("cargo:rustc-env=TEST_NETWORK_LAUNCHER_VERSION={version}");
    println!("cargo:rerun-if-changed=test-network-launcher-version");

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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=artifacts/mod.rs");
    println!("cargo:rerun-if-changed=artifacts/source.json");

    if option_env!("GIT_SHA").is_none() {
        define_git_sha();
    }
    define_test_network_launcher_version();
    artifacts::bundle_artifacts();
}
