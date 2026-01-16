use bigdecimal::BigDecimal;
use candid::Principal;
use icp::{prelude::*, project::DEFAULT_LOCAL_ENVIRONMENT_NAME};
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_DECIMALS;

use crate::common::TestContext;

pub(crate) struct Client<'a> {
    ctx: &'a TestContext,
    current_dir: PathBuf,
    environment: String,
}

impl<'a> Client<'a> {
    pub(crate) fn new(
        ctx: &'a TestContext,
        current_dir: PathBuf,
        environment: Option<String>,
    ) -> Self {
        Self {
            ctx,
            current_dir,
            environment: environment.unwrap_or(DEFAULT_LOCAL_ENVIRONMENT_NAME.to_string()),
        }
    }

    pub(crate) fn active_principal(&self) -> Principal {
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

    pub(crate) fn create_identity(&self, name: &str) {
        self.ctx
            .icp()
            .current_dir(&self.current_dir)
            .args(["identity", "new", name])
            .assert()
            .success();
    }

    pub(crate) fn get_principal(&self, name: &str) -> Principal {
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

    pub(crate) fn use_identity(&self, name: &str) {
        self.ctx
            .icp()
            .current_dir(&self.current_dir)
            .args(["identity", "default", name])
            .assert()
            .success();
    }

    pub(crate) fn use_new_random_identity(&self) -> Principal {
        let random_name = format!("alice-{}", rand::random::<u64>());
        self.create_identity(&random_name);
        self.use_identity(&random_name);
        self.active_principal()
    }

    pub(crate) fn mint_cycles(&self, amount: u128) {
        let tcycles = BigDecimal::new(amount.into(), CYCLES_LEDGER_DECIMALS);
        self.ctx
            .icp()
            .current_dir(&self.current_dir)
            .args([
                "cycles",
                "mint",
                "--tcycles",
                &tcycles.to_string(),
                "--environment",
                &self.environment,
            ])
            .assert()
            .success();
    }

    pub(crate) fn get_canister_id(&self, canister_name: &str) -> Principal {
        let output = self
            .ctx
            .icp()
            .current_dir(&self.current_dir)
            .args([
                "canister",
                "status",
                canister_name,
                "--environment",
                &self.environment,
                "-i",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();

        // Output format is: "{canister_id}"
        let output_str = String::from_utf8(output).unwrap();
        let output_str = output_str.trim();

        Principal::from_text(output_str).unwrap()
    }
}
