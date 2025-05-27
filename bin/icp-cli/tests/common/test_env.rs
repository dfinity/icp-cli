use crate::common::os::PATH_SEPARATOR;
use assert_cmd::Command;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::fs::create_dir_all;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub struct TestEnv {
    home_dir: TempDir,
    bin_dir: PathBuf,
    dfx_path: Option<PathBuf>,
    os_path: OsString,
}

impl TestEnv {
    pub fn new() -> Self {
        let home_dir = tempfile::tempdir().expect("failed to create temp home dir");
        let bin_dir = home_dir.path().join("bin");
        fs::create_dir(&bin_dir).expect("failed to create bin dir");
        let os_path = Self::build_os_path(&bin_dir);
        eprintln!(
            "Test environment home directory: {}",
            home_dir.path().display()
        );

        Self {
            home_dir,
            bin_dir,
            dfx_path: None,
            os_path,
        }
    }

    /// Sets up `dfx` in the test environment by copying the binary from $ICPTEST_DFX_PATH
    pub fn with_dfx(mut self) -> Self {
        let dfx_path = std::env::var_os("ICPTEST_DFX_PATH")
            .expect("ICPTEST_DFX_PATH must be set to use with_dfx()");
        let src = PathBuf::from(dfx_path);
        assert!(
            src.exists(),
            "ICPTEST_DFX_PATH points to non-existent file: {}",
            src.display()
        );

        let dest = self.bin_dir.join("dfx");
        fs::copy(&src, &dest).unwrap_or_else(|e| panic!("Failed to copy dfx to test bin dir: {e}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)
                .unwrap_or_else(|e| panic!("Failed to read metadata for {}: {}", dest.display(), e))
                .permissions();
            perms.set_mode(0o500);
            fs::set_permissions(&dest, perms).unwrap();
        }

        self.dfx_path = Some(dest);
        self
    }

    pub fn home_path(&self) -> &Path {
        self.home_dir.path()
    }

    #[allow(dead_code)]
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
    }

    fn build_os_path(bin_dir: &Path) -> OsString {
        let old_path = env::var_os("PATH").unwrap_or_default();
        let mut new_path = bin_dir.as_os_str().to_os_string();
        new_path.push(PATH_SEPARATOR);
        new_path.push(old_path);
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
}
