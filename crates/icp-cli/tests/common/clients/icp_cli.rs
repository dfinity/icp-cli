use candid::Principal;
use icp::prelude::PathBuf;

use crate::common::TestContext;

pub struct IcpCliClient<'a> {
    ctx: &'a TestContext,
    current_dir: &'a PathBuf,
}

impl<'a> IcpCliClient<'a> {
    pub fn new(ctx: &'a TestContext, current_dir: &'a PathBuf) -> Self {
        Self { ctx, current_dir }
    }

    pub fn active_principal(&self) -> Principal {
        let stdout = String::from_utf8(
            self.ctx
                .icp()
                .current_dir(self.current_dir)
                .args(["identity", "principal"])
                .assert()
                .get_output()
                .stdout
                .clone(),
        )
        .unwrap();
        Principal::from_text(stdout.trim()).unwrap()
    }

    pub fn create_identity(&self, name: &str) {
        self.ctx
            .icp()
            .current_dir(self.current_dir)
            .args(["identity", "new", name])
            .assert()
            .success();
    }

    pub fn get_principal(&self, name: &str) -> Principal {
        let stdout = String::from_utf8(
            self.ctx
                .icp()
                .current_dir(self.current_dir)
                .args(["identity", "principal", "--identity", name])
                .assert()
                .get_output()
                .stdout
                .clone(),
        )
        .unwrap();
        Principal::from_text(stdout.trim()).unwrap()
    }

    pub fn use_identity(&self, name: &str) {
        self.ctx
            .icp()
            .current_dir(self.current_dir)
            .args(["identity", "default", name])
            .assert()
            .success();
    }

    pub fn use_new_random_identity(&self) -> Principal {
        let random_name = format!("alice-{}", rand::random::<u64>());
        self.create_identity(&random_name);
        self.use_identity(&random_name);
        self.active_principal()
    }

    pub fn mint_cycles(&self, amount: u128) {
        self.ctx
            .icp()
            .current_dir(self.current_dir)
            .args(["cycles", "mint", "--cycles", &amount.to_string()])
            .assert()
            .success();
    }
}
