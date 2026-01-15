use std::{
    cell::OnceCell,
    env,
    ffi::OsString,
    fs::{self, create_dir_all},
};

use assert_cmd::Command;
use camino_tempfile::{Utf8TempDir as TempDir, tempdir};
use candid::Principal;
use ic_agent::Agent;
use icp::{
    directories::{Access, Directories},
    network::managed::{
        launcher::{NetworkInstance, wait_for_launcher_status},
        run::initialize_network,
    },
    prelude::*,
};
use url::Url;

use crate::common::{ChildGuard, PATH_SEPARATOR, TestNetwork};
pub(crate) struct TestContext {
    home_dir: TempDir,
    bin_dir: PathBuf,
    asset_dir: PathBuf,
    mock_cred_dir: PathBuf,
    os_path: OsString,
    gateway_url: OnceCell<Url>,
    root_key: OnceCell<Vec<u8>>,
}

impl TestContext {
    pub(crate) fn new() -> Self {
        // Home
        let home_dir = tempdir().expect("failed to create temp home dir");

        // Binaries
        let bin_dir = home_dir.path().join("bin");
        fs::create_dir(&bin_dir).expect("failed to create bin dir");

        // Assets
        let asset_dir = home_dir.path().join("share");
        fs::create_dir(&asset_dir).expect("failed to create asset dir");

        // Credentials
        let mock_cred_dir = home_dir.path().join("mock-keyring");
        fs::create_dir(&mock_cred_dir).expect("failed to create mock keyring dir");

        eprintln!("Test environment home directory: {}", home_dir.path());

        // OS Path
        let os_path = TestContext::build_os_path(&bin_dir);

        Self {
            home_dir,
            bin_dir,
            asset_dir,
            mock_cred_dir,
            os_path,
            gateway_url: OnceCell::new(),
            root_key: OnceCell::new(),
        }
    }

    pub(crate) fn home_path(&self) -> &Path {
        self.home_dir.path()
    }

    pub(crate) fn icp(&self) -> Command {
        let mut cmd = Command::cargo_bin("icp").expect("icp binary exists");

        // Isolate the command
        cmd.current_dir(self.home_path());
        cmd.env("HOME", self.home_path());
        cmd.env("PATH", self.os_path.clone());
        cmd.env_remove("ICP_HOME");
        cmd.env("ICP_CLI_KEYRING_MOCK_DIR", self.mock_cred_dir.clone());

        cmd
    }

    pub(crate) async fn launcher_path(&self) -> PathBuf {
        if let Ok(var) = env::var("ICP_CLI_NETWORK_LAUNCHER_PATH") {
            PathBuf::from(var)
        } else {
            // replicate the command's logic to only perform it if needed, and perform it in the user home instead of the test home
            let cache = Directories::new()
                .unwrap()
                .package_cache()
                .unwrap()
                .into_write()
                .await
                .unwrap();
            if let Some(path) = icp::network::managed::cache::get_cached_launcher_version(
                cache.as_ref().read(),
                "latest",
            )
            .unwrap()
            {
                path
            } else {
                let (_ver, path) = icp::network::managed::cache::download_launcher_version(
                    cache.as_ref(),
                    "latest",
                    &reqwest::Client::new(),
                )
                .await
                .unwrap();
                path
            }
        }
    }

    fn build_os_path(bin_dir: &Path) -> OsString {
        let old_path = env::var_os("PATH").unwrap_or_default();
        let mut new_path = bin_dir.as_os_str().to_owned();
        new_path.push(PATH_SEPARATOR);
        new_path.push(&old_path);
        new_path
    }

    pub(crate) fn pkg_dir(&self) -> PathBuf {
        env!("CARGO_MANIFEST_DIR").into()
    }

    fn asset_source_path(&self, name: &str) -> PathBuf {
        self.pkg_dir().join(format!("tests/assets/{name}"))
    }

    pub(crate) fn make_asset(&self, name: &str) -> PathBuf {
        let target = self.asset_dir.join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("failed to create asset parent directories");
        }
        fs::copy(self.asset_source_path(name), &target).expect("failed to copy asset");
        target
    }

    /// Copy an entire asset directory to the specified destination
    pub(crate) fn copy_asset_dir(&self, asset_name: &str, dest: &Path) {
        let source = self.asset_source_path(asset_name);
        if !source.exists() {
            panic!("Asset directory not found: {}", source);
        }
        Self::copy_dir_recursive(&source, dest);
    }

    fn copy_dir_recursive(src: &Path, dest: &Path) {
        fs::create_dir_all(dest).expect("failed to create destination directory");
        for entry in fs::read_dir(src.as_std_path()).expect("failed to read source directory") {
            let entry = entry.expect("failed to read directory entry");
            let std_path = entry.path();
            let file_name = std_path.file_name().expect("failed to get file name");
            let dest_path = dest.join(file_name.to_str().expect("invalid UTF-8 in filename"));

            if std_path.is_dir() {
                let src_path = PathBuf::try_from(std_path).expect("invalid UTF-8 in path");
                Self::copy_dir_recursive(&src_path, &dest_path);
            } else {
                fs::copy(&std_path, dest_path.as_std_path()).expect("failed to copy file");
            }
        }
    }
    pub(crate) fn create_project_dir(&self, name: &str) -> PathBuf {
        let project_dir = self.home_path().join(name);
        std::fs::create_dir_all(&project_dir).expect("Failed to create icp project directory");
        std::fs::write(project_dir.join("icp.yaml"), "").expect("Failed to write project file");
        project_dir
    }

    /// Calling this method more than once will panic.
    /// Calling this method after calling [TestContext::start_network_with_config] will panic.
    pub(crate) async fn start_network_in(&self, project_dir: &Path, name: &str) -> ChildGuard {
        let icp_path = env!("CARGO_BIN_EXE_icp");
        let mut cmd = std::process::Command::new(icp_path);
        cmd.current_dir(project_dir)
            .env("HOME", self.home_path())
            .env_remove("ICP_HOME")
            .arg("network")
            .arg("start")
            .arg(name);

        let launcher_path = self.launcher_path().await;
        cmd.env("ICP_CLI_NETWORK_LAUNCHER_PATH", launcher_path);

        eprintln!("Running network in {project_dir}");

        let child_guard = ChildGuard::spawn(&mut cmd).expect("failed to spawn icp network ");

        // "icp network start" will wait for the local network to be healthy,
        // but for now we need to wait for the descriptor to be created.
        let network_descriptor = self.wait_for_network_descriptor(project_dir, name);
        self.root_key
            .set(network_descriptor.root_key.clone())
            .expect("Root key should not be already initialized");
        self.gateway_url
            .set(
                format!("http://localhost:{}", network_descriptor.gateway_port)
                    .parse()
                    .unwrap(),
            )
            .expect("Gateway URL should not be already initialized");
        child_guard
    }

    pub(crate) fn state_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir
            .join(".icp")
            .join("cache")
            .join("networks")
            .join("local")
            .join("state")
    }

    /// Start a network with a custom number of application subnets.
    /// This bypasses the CLI and directly spawns the launcher with the specified flags.
    /// Calling this method more than once will panic.
    /// Calling this method after calling [TestContext::start_network_in] will panic.
    pub(crate) async fn start_network_with_flags(
        &self,
        project_dir: &Path,
        flags: &[&str],
    ) -> ChildGuard {
        let launcher_path = self.launcher_path().await;

        // Create network directory structure
        let network_dir = project_dir
            .join(".icp")
            .join("cache")
            .join("networks")
            .join("local");
        create_dir_all(&network_dir).expect("Failed to create network directory");

        let launcher_dir = network_dir.join("network-launcher");
        create_dir_all(&launcher_dir).expect("Failed to create network launcher directory");

        let state_dir = network_dir.join("state");
        create_dir_all(&state_dir).expect("Failed to create state directory");

        eprintln!("Starting network with custom flags");

        // Spawn launcher
        let mut cmd = std::process::Command::new(&launcher_path);
        cmd.args(["--interface-version=1.0.0", "--status-dir"]);
        cmd.arg(&launcher_dir);
        cmd.args(flags);
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let watcher =
            wait_for_launcher_status(&launcher_dir).expect("Failed to watch launcher status");
        let guard = ChildGuard::spawn(&mut cmd).expect("Failed to spawn network launcher");
        let child = &guard.child;
        let launcher_pid = child.id();

        // Wait for port file using the function from icp-network
        let status = watcher.await.expect("Timeout waiting for port file");
        let gateway_port = status.gateway_port;
        eprintln!("Gateway started on port {gateway_port}");

        let instance = NetworkInstance {
            gateway_port,
            root_key: hex::decode(&status.root_key).unwrap(),
            pocketic_config_port: status.config_port,
            pocketic_instance_id: status.instance_id,
        };
        // Initialize network instance
        initialize_network(
            &format!("http://localhost:{}", instance.gateway_port)
                .parse()
                .unwrap(),
            &instance.root_key,
            [Principal::anonymous()], // Seed anonymous account only for tests
            None,
        )
        .await
        .expect("Failed to initialize network instance");

        // Build and write network descriptor
        let descriptor_path = network_dir.join("descriptor.json");
        let network_descriptor = serde_json::json!({
            "v": "1",
            "id": ::uuid::Uuid::new_v4().to_string(),
            "project-dir": project_dir.as_str(),
            "network": "local",
            "network-dir": network_dir.as_str(),
            "gateway": {
                "port": instance.gateway_port,
                "fixed": false
            },
            "child-locator": {
                "type": "pid",
                "pid": launcher_pid
            },
            "root-key": hex::encode(&instance.root_key),
        });
        fs::write(
            &descriptor_path,
            serde_json::to_string_pretty(&network_descriptor).unwrap(),
        )
        .expect("Failed to write network descriptor");

        self.root_key
            .set(instance.root_key.clone())
            .expect("Root key should not be already initialized");
        self.gateway_url
            .set(
                format!("http://localhost:{}", instance.gateway_port)
                    .parse()
                    .unwrap(),
            )
            .expect("Gateway URL should not be already initialized");
        guard
    }

    pub(crate) fn ping_until_healthy(&self, project_dir: &Path, name: &str) {
        self.wait_for_network_descriptor(project_dir, name);
        self.icp()
            .current_dir(project_dir)
            .args(["network", "ping", "--wait-healthy", name])
            .assert()
            .success();
    }

    // wait up for descriptor path to contain valid json
    pub(crate) fn wait_for_local_network_descriptor(&self, project_dir: &Path) -> TestNetwork {
        self.wait_for_network_descriptor(project_dir, "local")
    }

    pub(crate) fn wait_for_network_descriptor(
        &self,
        project_dir: &Path,
        network_name: &str,
    ) -> TestNetwork {
        let descriptor_path = project_dir
            .join(".icp")
            .join("cache")
            .join("networks")
            .join(network_name)
            .join("descriptor.json");
        let start_time = std::time::Instant::now();
        let timeout = 45;
        eprintln!("Waiting for network descriptor at {descriptor_path} - limit {timeout}s");
        let network_descriptor = loop {
            let elapsed = start_time.elapsed().as_secs();
            if descriptor_path.exists() && descriptor_path.is_file() {
                let contents = fs::read_to_string(&descriptor_path)
                    .expect("Failed to read network descriptor");
                let parsed = serde_json::from_str::<serde_json::Value>(&contents);
                if let Ok(value) = parsed {
                    eprintln!("Network descriptor found at {descriptor_path} after {elapsed}s");
                    break value;
                } else {
                    eprintln!(
                        "Network descriptor at {descriptor_path} is not valid JSON: {contents}"
                    );
                }
            }
            if elapsed > timeout {
                panic!(
                    "Timed out waiting for network descriptor at {descriptor_path} after {elapsed}s"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };

        let gateway_port: u16 = network_descriptor
            .get("gateway")
            .and_then(|g| g.get("port"))
            .and_then(|p| p.as_u64())
            .expect("network descriptor does not contain gateway port")
            as u16;

        let root_key = network_descriptor
            .get("root-key")
            .and_then(|rk| rk.as_str())
            .expect("network descriptor does not contain root key")
            .to_string();
        let root_key = hex::decode(root_key).unwrap();

        TestNetwork {
            gateway_port,
            root_key,
        }
    }

    fn network_descriptor_path(&self, project_dir: &Path, network: &str) -> PathBuf {
        project_dir
            .join(".icp")
            .join("cache")
            .join("networks")
            .join(network)
            .join("descriptor.json")
    }

    pub(crate) fn read_network_descriptor(&self, project_dir: &Path, network: &str) -> Vec<u8> {
        std::fs::read(self.network_descriptor_path(project_dir, network))
            .expect("Failed to read network descriptor file")
    }

    pub(crate) fn write_network_descriptor(
        &self,
        project_dir: &Path,
        network: &str,
        contents: &[u8],
    ) {
        let descriptor_path = self.network_descriptor_path(project_dir, network);
        std::fs::write(&descriptor_path, contents)
            .expect("Failed to write network descriptor file");
    }

    pub(crate) fn agent(&self) -> Agent {
        let agent = Agent::builder()
            .with_url(self.gateway_url.get().unwrap().as_str())
            .build()
            .unwrap();
        agent.set_root_key(self.root_key.get().unwrap().clone());
        agent
    }

    pub(crate) fn docker_pull_network(&self) {
        Command::new("docker")
            .args(["pull", "ghcr.io/dfinity/icp-cli-network-launcher:v11.0.0"])
            .assert()
            .success();
    }
}
