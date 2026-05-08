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

#[cfg(unix)]
fn download_test_network_launcher(version: &str, out_dir: &str) {
    use std::os::unix::fs::PermissionsExt;

    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let os = if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let pkg_version = format!("v{version}");
    let binary_path = format!("{out_dir}/icp-cli-network-launcher-{version}");

    if !std::path::Path::new(&binary_path).exists() {
        let tarball_name =
            format!("icp-cli-network-launcher-{arch}-{os}-{pkg_version}");
        let url = format!(
            "https://github.com/dfinity/icp-cli-network-launcher/releases/download/{pkg_version}/{tarball_name}.tar.gz"
        );
        eprintln!("Downloading test network launcher {pkg_version} from: {url}");

        let response = reqwest::blocking::get(&url)
            .unwrap_or_else(|e| panic!("failed to download network launcher: {e}"));
        if !response.status().is_success() {
            panic!("failed to download network launcher: HTTP {}", response.status());
        }
        let bytes = response
            .bytes()
            .unwrap_or_else(|e| panic!("failed to read response: {e}"));

        let tarball_path = format!("{out_dir}/launcher-{version}.tar.gz");
        std::fs::write(&tarball_path, &bytes).expect("failed to write tarball");

        let status = std::process::Command::new("tar")
            .args(["-xzf", &tarball_path, "-C", out_dir])
            .status()
            .expect("failed to run tar");
        assert!(status.success(), "tar extraction failed");

        let extracted =
            format!("{out_dir}/{tarball_name}/icp-cli-network-launcher");
        std::fs::rename(&extracted, &binary_path).expect("failed to move binary");
        let _ = std::fs::remove_dir_all(format!("{out_dir}/{tarball_name}"));

        let mut perms = std::fs::metadata(&binary_path)
            .expect("failed to stat binary")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary_path, perms)
            .expect("failed to set permissions");

        let _ = std::fs::remove_file(&tarball_path);
    }

    println!("cargo:rustc-env=ICP_CLI_NETWORK_LAUNCHER_PATH={binary_path}");
}

fn define_test_network_launcher_version() {
    let raw = std::fs::read_to_string("test-network-launcher-version")
        .expect("missing test-network-launcher-version file");
    let version = raw.trim().trim_start_matches('v');
    println!("cargo:rustc-env=TEST_NETWORK_LAUNCHER_VERSION={version}");
    println!("cargo:rerun-if-changed=test-network-launcher-version");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    #[cfg(unix)]
    download_test_network_launcher(version, &out_dir);

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
