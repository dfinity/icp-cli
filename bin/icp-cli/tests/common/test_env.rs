use assert_cmd::Command;
use std::path::Path;
use tempfile::TempDir;

pub struct TestEnv {
    home_dir: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        Self {
            home_dir: tempfile::tempdir().expect("create home dir for test"),
        }
    }

    pub fn home_path(&self) -> &Path {
        self.home_dir.path()
    }

    pub fn icp(&self) -> Command {
        let mut cmd = Command::cargo_bin("icp").expect("binary exists");
        cmd.env("HOME", self.home_path());
        cmd
    }
}
