use std::{
    cell::{Cell, OnceCell},
    env,
    ffi::OsString,
    fs,
    time::Duration,
};

use assert_cmd::{Command, cargo::cargo_bin_cmd};
use camino_tempfile::{Utf8TempDir as TempDir, tempdir};
use ic_agent::Agent;
use icp::prelude::*;
use reqwest::Client;
use serde_json::json;
use time::UtcDateTime;
use url::Url;

use crate::common::{ChildGuard, PATH_SEPARATOR, TestNetwork, softhsm::SoftHsmContext};

pub(crate) struct TestContext {
    home_dir: TempDir,
    bin_dir: PathBuf,
    asset_dir: PathBuf,
    mock_cred_dir: PathBuf,
    os_path: OsString,
    gateway_url: OnceCell<Url>,
    config_url: OnceCell<Option<Url>>,
    time_offset: Cell<Option<Duration>>,
    root_key: OnceCell<Vec<u8>>,
    softhsm: OnceCell<SoftHsmContext>,
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

        // App files
        let icp_home_dir = home_dir.path().join("icp");
        fs::create_dir(&icp_home_dir).expect("failed to create icp home dir");

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
            config_url: OnceCell::new(),
            root_key: OnceCell::new(),
            softhsm: OnceCell::new(),
            time_offset: Cell::new(None),
        }
    }

    pub(crate) fn home_path(&self) -> &Path {
        self.home_dir.path()
    }

    /// Initialize SoftHSM for this test context.
    ///
    /// This creates an isolated SoftHSM environment with a test token and key.
    /// All subsequent `icp()` calls will include the SOFTHSM2_CONF environment
    /// variable pointing to this context's configuration.
    ///
    /// Can only be called once per TestContext.
    pub(crate) fn init_softhsm(&self) -> &SoftHsmContext {
        self.softhsm.get_or_init(SoftHsmContext::new)
    }

    pub(crate) fn icp(&self) -> Command {
        #[allow(clippy::disallowed_types)]
        let mut cmd = cargo_bin_cmd!("icp");

        // Isolate the command
        cmd.current_dir(self.home_path());
        // Isolate the whole user directory in Unix, test in normal mode
        #[cfg(unix)]
        cmd.env("HOME", self.home_path()).env_remove("ICP_HOME");
        // Run in portable mode on Windows, the user directory cannot be mocked
        #[cfg(windows)]
        cmd.env("ICP_HOME", self.home_path().join("icp"));
        cmd.env("PATH", self.os_path.clone());
        cmd.env("ICP_CLI_KEYRING_MOCK_DIR", self.mock_cred_dir.clone());

        // If SoftHSM has been initialized, include its config
        if let Some(hsm) = self.softhsm.get() {
            cmd.env("SOFTHSM2_CONF", &hsm.config_path);
        }

        if let Some(offset) = self.time_offset.get() {
            cmd.env(
                "ICP_CLI_TEST_ADVANCE_TIME_MS",
                offset.as_millis().to_string(),
            );
        }

        cmd
    }

    #[cfg(unix)]
    pub(crate) async fn launcher_path(&self) -> PathBuf {
        use icp::directories::{Access, Directories};
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

    pub(crate) async fn launcher_path_or_nothing(&self) -> PathBuf {
        #[cfg(unix)]
        {
            self.launcher_path().await
        }
        #[cfg(windows)]
        {
            PathBuf::new()
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
        cmd.current_dir(project_dir);
        // isolate the whole user directory in Unix, test in normal mode
        #[cfg(unix)]
        cmd.env("HOME", self.home_path()).env_remove("ICP_HOME");
        // run in portable mode on Windows, the user directory cannot be mocked
        #[cfg(windows)]
        cmd.env("ICP_HOME", self.home_path().join("icp"));
        cmd.arg("network").arg("start").arg(name);
        #[cfg(unix)]
        {
            let launcher_path = self.launcher_path().await;
            cmd.env("ICP_CLI_NETWORK_LAUNCHER_PATH", launcher_path);
        }

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
        self.config_url
            .set(network_descriptor.pocketic_config_port.and_then(|port| {
                network_descriptor.pocketic_instance_id.map(|instance| {
                    format!("http://localhost:{port}/instances/{instance}/")
                        .parse()
                        .expect("Failed to parse PocketIC config URL")
                })
            }))
            .expect("Config URL should not be already initialized");
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
        let timeout = 300;
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
        let pocketic_config_port: Option<u16> = network_descriptor
            .get("pocketic-config-port")
            .and_then(|p| p.as_u64())
            .map(|p| p as u16);
        let pocketic_instance_id: Option<usize> = network_descriptor
            .get("pocketic-instance-id")
            .and_then(|p| p.as_u64())
            .map(|p| p as usize);

        let root_key = network_descriptor
            .get("root-key")
            .and_then(|rk| rk.as_str())
            .expect("network descriptor does not contain root key")
            .to_string();
        let root_key = hex::decode(root_key).unwrap();

        TestNetwork {
            gateway_port,
            root_key,
            pocketic_config_port,
            pocketic_instance_id,
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

    pub(crate) async fn pocketic_time_fastforward(&self, duration: Duration) {
        let now = UtcDateTime::now();
        let url = self
            .config_url
            .get()
            .expect("network must have been initialized")
            .as_ref()
            .expect("network must use pocket-ic")
            .join("update/set_time")
            .unwrap();
        let body = json!({ "nanos_since_epoch": (now + duration).unix_timestamp_nanos() });
        for attempt in 0..5 {
            let response = Client::new()
                .post(url.clone())
                .json(&body)
                .send()
                .await
                .expect("failed to update time");
            if response.status() == reqwest::StatusCode::CONFLICT {
                tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
                continue;
            }
            response.error_for_status().expect("failed to update time");
            self.time_offset.set(Some(duration));
            return;
        }
        panic!("failed to update time: still receiving 409 Conflict after 5 attempts");
    }

    pub(crate) fn pocketic_config_url(&self) -> Option<&Url> {
        self.config_url
            .get()
            .expect("network must have been initialized")
            .as_ref()
    }

    pub(crate) fn docker_pull_network(&self) {
        let platform = if cfg!(target_arch = "aarch64") {
            "linux/arm64"
        } else {
            "linux/amd64"
        };
        Command::new("docker")
            .args([
                "pull",
                "--platform",
                platform,
                "ghcr.io/dfinity/icp-cli-network-launcher:v11.0.0",
            ])
            .assert()
            .success();
    }
}
