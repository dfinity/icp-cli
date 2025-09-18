use std::{
    cell::{Ref, RefCell},
    env,
    ffi::OsString,
    fs::{self, create_dir_all},
};

use assert_cmd::Command;
use camino::{Utf8Path, Utf8PathBuf};
use camino_tempfile::{Utf8TempDir, tempdir};
use icp_network::NETWORK_LOCAL;
use pocket_ic::nonblocking::PocketIc;
use serde_json::{Value, json};
use url::Url;

use crate::common::{
    ChildGuard, PATH_SEPARATOR, TestNetwork, TestNetworkForDfx,
    context::{
        cycles_ledger::CyclesLedgerPocketIcClient, icp_ledger::IcpLedgerPocketIcClient,
        icp_shorthand::IcpShorthand,
    },
};

mod cycles_ledger;
mod icp_ledger;
mod icp_shorthand;
pub struct TestContext {
    home_dir: Utf8TempDir,
    bin_dir: Utf8PathBuf,
    asset_dir: Utf8PathBuf,
    dfx_path: Option<Utf8PathBuf>,
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
            dfx_path: None,
            os_path,
            pocketic: RefCell::new(None),
        }
    }

    /// Sets up `dfx` in the test environment by copying the binary from $ICPTEST_DFX_PATH
    pub fn with_dfx(mut self) -> Self {
        let dfx_path = std::env::var("ICPTEST_DFX_PATH")
            .expect("ICPTEST_DFX_PATH must be set to use with_dfx()");

        let src = Utf8PathBuf::from(dfx_path);
        assert!(
            src.exists(),
            "ICPTEST_DFX_PATH points to non-existent file: {src}",
        );

        let dest = self.bin_dir.join("dfx");
        fs::copy(&src, &dest).unwrap_or_else(|e| panic!("Failed to copy dfx to test bin dir: {e}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)
                .unwrap_or_else(|e| panic!("Failed to read metadata for {dest}: {e}"))
                .permissions();
            perms.set_mode(0o500);
            fs::set_permissions(&dest, perms).unwrap();
        }

        self.dfx_path = Some(dest);
        self
    }

    pub fn home_path(&self) -> &Utf8Path {
        self.home_dir.path()
    }

    pub fn icp(&self) -> Command {
        let mut cmd = Command::cargo_bin("icp").expect("icp binary exists");
        self.isolate(&mut cmd);
        cmd
    }

    pub fn dfx(&self) -> Command {
        let dfx_path = self
            .dfx_path
            .as_ref()
            .expect("dfx not configured in test env — call with_dfx() first");
        let mut cmd = Command::new(dfx_path);
        self.isolate(&mut cmd);
        cmd
    }

    fn isolate(&self, cmd: &mut Command) {
        cmd.current_dir(self.home_path());
        cmd.env("HOME", self.home_path());
        cmd.env("PATH", self.os_path.clone());
        cmd.env_remove("ICP_HOME");
    }

    fn build_os_path(bin_dir: &Utf8Path) -> OsString {
        let old_path = env::var_os("PATH").unwrap_or_default();
        let mut new_path = bin_dir.as_os_str().to_owned();
        new_path.push(PATH_SEPARATOR);
        new_path.push(&old_path);
        new_path
    }

    pub fn configure_dfx_local_network(&self) {
        let dfx_config_dir = self.home_path().join(".config").join("dfx");
        create_dir_all(&dfx_config_dir).expect("create .config directory");
        let networks_json_path = dfx_config_dir.join("networks.json");

        let bind_address = "127.0.0.1:8000";
        let networks = format!(r#"{{"local": {{"bind": "{}"}}}}"#, bind_address);
        fs::write(&networks_json_path, networks).unwrap();
    }

    pub fn configure_dfx_network(
        &self,
        icp_project_dir: &Utf8Path,
        network_name: &str,
    ) -> TestNetworkForDfx {
        let test_network = self.wait_for_network_descriptor(icp_project_dir, network_name);

        let dfx_network_name = format!("{}-{}", icp_project_dir.file_name().unwrap(), network_name);
        let gateway_port = test_network.gateway_port;

        let dfx_config_dir = self.home_path().join(".config").join("dfx");
        create_dir_all(&dfx_config_dir).expect("create .config directory");
        let networks_json_path = dfx_config_dir.join("networks.json");

        // Build the bind address
        let bind_address = format!("127.0.0.1:{gateway_port}");

        // Create the inner object
        let network_entry = json!({
            "bind": bind_address,
        });

        // Construct the outer object dynamically
        let mut root = serde_json::Map::new();
        root.insert(dfx_network_name.to_string(), network_entry);

        let networks = serde_json::to_string_pretty(&Value::Object(root)).unwrap();

        eprintln!("Configuring dfx network: {}", networks);
        fs::write(&networks_json_path, networks).unwrap();
        TestNetworkForDfx {
            dfx_network_name,
            gateway_port,
        }
    }

    pub fn pkg_dir(&self) -> Utf8PathBuf {
        env!("CARGO_MANIFEST_DIR").into()
    }

    pub fn make_asset(&self, name: &str) -> Utf8PathBuf {
        let target = self.asset_dir.join(name);
        fs::copy(self.pkg_dir().join(format!("tests/assets/{name}")), &target)
            .expect("failed to copy asset");
        target
    }
    pub fn create_project_dir(&self, name: &str) -> Utf8PathBuf {
        let project_dir = self.home_path().join(name);
        std::fs::create_dir_all(&project_dir).expect("Failed to create icp project directory");
        std::fs::write(project_dir.join("icp.yaml"), "").expect("Failed to write project file");
        project_dir
    }

    pub fn start_network_in(&self, project_dir: &Utf8Path) -> ChildGuard {
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

    pub fn ping_until_healthy(&self, project_dir: &Utf8Path) {
        self.wait_for_local_network_descriptor(project_dir);
        self.icp()
            .current_dir(project_dir)
            .args(["network", "ping", "--wait-healthy"])
            .assert()
            .success();
    }

    // wait up to 30 seconds for descriptor path to contain valid json
    pub fn wait_for_local_network_descriptor(&self, project_dir: &Utf8Path) -> TestNetwork {
        self.wait_for_network_descriptor(project_dir, NETWORK_LOCAL)
    }

    pub fn wait_for_network_descriptor(
        &self,
        project_dir: &Utf8Path,
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

    pub fn configure_icp_local_network_random_port(&self, project_dir: &Utf8Path) {
        self.configure_icp_local_network_port(project_dir, 0);
    }

    pub fn configure_icp_local_network_port(&self, project_dir: &Utf8Path, gateway_port: u16) {
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

    fn network_descriptor_path(&self, project_dir: &Utf8Path, network: &str) -> Utf8PathBuf {
        project_dir
            .join(".icp")
            .join("networks")
            .join(network)
            .join("descriptor.json")
    }

    pub fn read_network_descriptor(&self, project_dir: &Utf8Path, network: &str) -> Vec<u8> {
        std::fs::read(self.network_descriptor_path(project_dir, network))
            .expect("Failed to read network descriptor file")
    }

    pub fn write_network_descriptor(&self, project_dir: &Utf8Path, network: &str, contents: &[u8]) {
        let descriptor_path = self.network_descriptor_path(project_dir, network);
        std::fs::write(&descriptor_path, contents)
            .expect("Failed to write network descriptor file");
    }

    pub fn icp_(&self) -> IcpShorthand<'_> {
        IcpShorthand::new(self)
    }

    pub fn icp_ledger(&self) -> IcpLedgerPocketIcClient<'_> {
        let pic_ref: Ref<'_, PocketIc> = Ref::map(self.pocketic.borrow(), |opt| {
            opt.as_ref().expect("pocketic not started")
        });
        IcpLedgerPocketIcClient { pic: pic_ref }
    }

    pub fn cycles_ledger(&self) -> CyclesLedgerPocketIcClient<'_> {
        let pic_ref: Ref<'_, PocketIc> = Ref::map(self.pocketic.borrow(), |opt| {
            opt.as_ref().expect("pocketic not started")
        });
        CyclesLedgerPocketIcClient { pic: pic_ref }
    }
}
