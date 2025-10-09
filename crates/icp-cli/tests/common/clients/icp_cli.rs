use candid::Principal;
use icp::prelude::*;

use crate::common::TestContext;

pub struct Client<'a> {
    ctx: &'a TestContext,
    current_dir: PathBuf,
    environment: String,
}

impl<'a> Client<'a> {
    pub fn new(ctx: &'a TestContext, current_dir: PathBuf, environment: Option<String>) -> Self {
        Self {
            ctx,
            current_dir,
            environment: environment.unwrap_or("local".to_string()),
        }
    }

    pub fn active_principal(&self) -> Principal {
        let stdout = String::from_utf8(
            self.ctx
                .icp()
                .current_dir(&self.current_dir)
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
            .current_dir(&self.current_dir)
            .args(["identity", "new", name])
            .assert()
            .success();
    }

    pub fn get_principal(&self, name: &str) -> Principal {
        let stdout = String::from_utf8(
            self.ctx
                .icp()
                .current_dir(&self.current_dir)
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
            .current_dir(&self.current_dir)
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
            .current_dir(&self.current_dir)
            .args([
                "cycles",
                "mint",
                "--cycles",
                &amount.to_string(),
                "--environment",
                &self.environment,
            ])
            .assert()
            .success();
    }

    pub fn get_canister_id(&self, canister_name: &str) -> Principal {
        let output = self
            .ctx
            .icp()
            .current_dir(&self.current_dir)
            .args([
                "canister",
                "show",
                canister_name,
                "--environment",
                &self.environment,
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let output_str = String::from_utf8(output).unwrap();

        // Output format is: "{canister_id} => {canister_info}"
        let id_str = output_str
            .split(" => ")
            .next()
            .expect("Failed to parse canister show output")
            .trim();
        Principal::from_text(id_str).unwrap()
    }
}
