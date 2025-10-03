use assert_cmd::Command;
use camino_tempfile::{Utf8TempDir as TempDir, tempdir};
use candid::Principal;
use icp::prelude::*;
use icp_network::NETWORK_LOCAL;
use icp_network::managed::pocketic;
use pocket_ic::nonblocking::PocketIc;
use std::{
    cell::{Ref, RefCell},
    env,
    ffi::OsString,
    fs::{self, create_dir_all},
};
use url::Url;

use crate::common::{ChildGuard, PATH_SEPARATOR, TestNetwork};
pub struct TestContext {
    home_dir: TempDir,
    bin_dir: PathBuf,
    asset_dir: PathBuf,
    os_path: OsString,
    pocketic: RefCell<Option<PocketIc>>,
}

impl TestContext {
    pub fn new() -> Self {
        // Home
        let home_dir = tempdir().expect("failed to create temp home dir");

        // Binaries
        let bin_dir = home_dir.path().join("bin");
        fs::create_dir(&bin_dir).expect("failed to create bin dir");

        // Assets
        let asset_dir = home_dir.path().join("share");
        fs::create_dir(&asset_dir).expect("failed to create asset dir");

        eprintln!("Test environment home directory: {}", home_dir.path());

        // OS Path
        let os_path = TestContext::build_os_path(&bin_dir);

        Self {
            home_dir,
            bin_dir,
            asset_dir,
            os_path,
            pocketic: RefCell::new(None),
        }
    }

    pub fn home_path(&self) -> &Path {
        self.home_dir.path()
    }

    pub fn icp(&self) -> Command {
        let mut cmd = Command::cargo_bin("icp").expect("icp binary exists");
        self.isolate(&mut cmd);
        cmd
    }

    fn isolate(&self, cmd: &mut Command) {
        cmd.current_dir(self.home_path());
        cmd.env("HOME", self.home_path());
        cmd.env("PATH", self.os_path.clone());
        cmd.env_remove("ICP_HOME");
    }

    fn build_os_path(bin_dir: &Path) -> OsString {
        let old_path = env::var_os("PATH").unwrap_or_default();
        let mut new_path = bin_dir.as_os_str().to_owned();
        new_path.push(PATH_SEPARATOR);
        new_path.push(&old_path);
        new_path
    }

    pub fn pkg_dir(&self) -> PathBuf {
        env!("CARGO_MANIFEST_DIR").into()
    }

    pub fn make_asset(&self, name: &str) -> PathBuf {
        let target = self.asset_dir.join(name);
        fs::copy(self.pkg_dir().join(format!("tests/assets/{name}")), &target)
            .expect("failed to copy asset");
        target
    }
    pub fn create_project_dir(&self, name: &str) -> PathBuf {
        let project_dir = self.home_path().join(name);
        std::fs::create_dir_all(&project_dir).expect("Failed to create icp project directory");
        std::fs::write(project_dir.join("icp.yaml"), "").expect("Failed to write project file");
        project_dir
    }

    pub fn start_network_in(&self, project_dir: &Path) -> ChildGuard {
        let icp_path = env!("CARGO_BIN_EXE_icp");
        let mut cmd = std::process::Command::new(icp_path);
        cmd.current_dir(project_dir)
            .env("HOME", self.home_path())
            .env_remove("ICP_HOME")
            .arg("network")
            .arg("run");

        eprintln!("Running network in {}", project_dir);

        let child_guard = ChildGuard::spawn(&mut cmd).expect("failed to spawn icp network ");

        // "icp network start" will wait for the local network to be healthy,
        // but for now we need to wait for the descriptor to be created.
        let network_descriptor = self.wait_for_local_network_descriptor(project_dir);
        let pocketic = PocketIc::new_from_existing_instance(
            network_descriptor.pocketic_url,
            network_descriptor.pocketic_instance_id,
            None,
        );
        self.pocketic.replace(Some(pocketic));

        child_guard
    }

    /// Start a network with a custom number of application subnets.
    /// This bypasses the CLI and directly spawns PocketIC with the specified configuration.
    pub async fn start_network_with_subnets(
        &self,
        project_dir: &Path,
        application_subnets: usize,
    ) -> ChildGuard {
        let pocketic_path =
            PathBuf::from(env::var("ICP_POCKET_IC_PATH").expect("ICP_POCKET_IC_PATH must be set"));

        // Create network directory structure
        let network_dir = project_dir
            .join(".icp")
            .join("networks")
            .join(NETWORK_LOCAL);
        create_dir_all(&network_dir).expect("Failed to create network directory");

        let pocketic_dir = network_dir.join("pocketic");
        create_dir_all(&pocketic_dir).expect("Failed to create pocketic directory");

        let state_dir = pocketic_dir.join("state");
        create_dir_all(&state_dir).expect("Failed to create state directory");

        let port_file = pocketic_dir.join("port");

        eprintln!(
            "Starting PocketIC with {} application subnet(s)",
            application_subnets
        );

        // Spawn PocketIC
        let mut cmd = std::process::Command::new(&pocketic_path);
        cmd.arg("--port-file");
        cmd.arg(&port_file);
        cmd.args(["--ttl", "2592000", "--log-levels", "error"]);
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let child = cmd.spawn().expect("failed to spawn PocketIC");
        let pocketic_pid = child.id();

        // Wait for port file
        let pocketic_port = self.wait_for_port_file(&port_file);
        eprintln!("PocketIC started on port {}", pocketic_port);

        // Initialize PocketIC instance with custom config
        let inst_cfg = pocketic::custom_instance_config(&state_dir, application_subnets);
        let instance = pocketic::initialize_instance(
            pocketic_port,
            inst_cfg,
            None,                                    // Random gateway port
            std::iter::once(Principal::anonymous()), // Seed anonymous account only for tests
        )
        .await
        .expect("Failed to initialize PocketIC instance");

        // Build and write network descriptor
        let descriptor_path = network_dir.join("descriptor.json");
        let network_descriptor = serde_json::json!({
            "id": ::uuid::Uuid::new_v4().to_string(),
            "project-dir": project_dir.as_str(),
            "network": NETWORK_LOCAL,
            "network-dir": network_dir.as_str(),
            "gateway": {
                "port": instance.gateway_port,
                "fixed": false
            },
            "default-effective-canister-id": instance.effective_canister_id.to_string(),
            "pocketic-url": format!("http://localhost:{}", pocketic_port),
            "pocketic-instance-id": instance.instance_id,
            "pid": pocketic_pid,
            "root-key": instance.root_key,
        });
        fs::write(
            &descriptor_path,
            serde_json::to_string_pretty(&network_descriptor).unwrap(),
        )
        .expect("Failed to write network descriptor");

        // Connect PocketIc client
        let pocketic = PocketIc::new_from_existing_instance(
            format!("http://localhost:{}", pocketic_port)
                .parse()
                .unwrap(),
            instance.instance_id,
            None,
        );
        self.pocketic.replace(Some(pocketic));

        // Wrap child in ChildGuard
        ChildGuard { child }
    }

    fn wait_for_port_file(&self, port_file: &Path) -> u16 {
        let mut retries = 0;
        while retries < 300 {
            if let Ok(contents) = fs::read_to_string(port_file) {
                if contents.ends_with('\n') {
                    if let Ok(port) = contents.trim().parse::<u16>() {
                        return port;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            retries += 1;
        }
        panic!("Timeout waiting for port file");
    }

    pub fn ping_until_healthy(&self, project_dir: &Path) {
        self.wait_for_local_network_descriptor(project_dir);
        self.icp()
            .current_dir(project_dir)
            .args(["network", "ping", "--wait-healthy"])
            .assert()
            .success();
    }

    // wait up to 30 seconds for descriptor path to contain valid json
    pub fn wait_for_local_network_descriptor(&self, project_dir: &Path) -> TestNetwork {
        self.wait_for_network_descriptor(project_dir, NETWORK_LOCAL)
    }

    pub fn wait_for_network_descriptor(
        &self,
        project_dir: &Path,
        network_name: &str,
    ) -> TestNetwork {
        let descriptor_path = project_dir
            .join(".icp")
            .join("networks")
            .join(network_name)
            .join("descriptor.json");
        let start_time = std::time::Instant::now();
        let network_descriptor = loop {
            eprintln!("Checking for network descriptor at {}", descriptor_path);
            if descriptor_path.exists() && descriptor_path.is_file() {
                let contents = fs::read_to_string(&descriptor_path)
                    .expect("Failed to read network descriptor");
                let parsed = serde_json::from_str::<serde_json::Value>(&contents);
                if let Ok(value) = parsed {
                    eprintln!("Network descriptor found at {}", descriptor_path);
                    break value;
                } else {
                    eprintln!(
                        "Network descriptor at {} is not valid JSON: {}",
                        descriptor_path, contents
                    );
                }
            }
            if start_time.elapsed().as_secs() > 30 {
                panic!(
                    "Timed out waiting for network descriptor at {}",
                    descriptor_path
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

        let pocketic_url = network_descriptor
            .get("pocketic-url")
            .and_then(|pu| pu.as_str())
            .expect("network descriptor does not contain pocketic url")
            .to_string();
        let pocketic_url = Url::parse(&pocketic_url).expect("invalid pocketic url");

        let pocketic_instance_id = network_descriptor
            .get("pocketic-instance-id")
            .and_then(|pii| pii.as_u64())
            .expect("network descriptor does not contain pocketic instance id")
            as usize;

        TestNetwork {
            gateway_port,
            root_key,
            pocketic_url,
            pocketic_instance_id,
        }
    }

    pub fn configure_icp_local_network_random_port(&self, project_dir: &Path) {
        self.configure_icp_local_network_port(project_dir, 0);
    }

    pub fn configure_icp_local_network_port(&self, project_dir: &Path, gateway_port: u16) {
        let networks_dir = project_dir.join("networks");
        create_dir_all(&networks_dir).expect("Failed to create networks directory");
        fs::write(
            networks_dir.join("local.yaml"),
            format!(
                r#"
        mode: managed
        gateway:
          port: {gateway_port}
        "#
            ),
        )
        .unwrap();
    }

    fn network_descriptor_path(&self, project_dir: &Path, network: &str) -> PathBuf {
        project_dir
            .join(".icp")
            .join("networks")
            .join(network)
            .join("descriptor.json")
    }

    pub fn read_network_descriptor(&self, project_dir: &Path, network: &str) -> Vec<u8> {
        std::fs::read(self.network_descriptor_path(project_dir, network))
            .expect("Failed to read network descriptor file")
    }

    pub fn write_network_descriptor(&self, project_dir: &Path, network: &str, contents: &[u8]) {
        let descriptor_path = self.network_descriptor_path(project_dir, network);
        std::fs::write(&descriptor_path, contents)
            .expect("Failed to write network descriptor file");
    }

    pub fn pocketic(&self) -> Ref<'_, PocketIc> {
        Ref::map(self.pocketic.borrow(), |opt| {
            opt.as_ref().expect("PocketIc instance not initialized")
        })
    }
}
