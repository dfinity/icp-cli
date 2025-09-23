use candid::Principal;

use crate::common::TestContext;

pub struct IcpCliClient<'a> {
    ctx: &'a TestContext,
}

impl<'a> IcpCliClient<'a> {
    pub fn new(ctx: &'a TestContext) -> Self {
        Self { ctx }
    }

    pub fn active_principal(&self) -> Principal {
        let stdout = String::from_utf8(
            self.ctx
                .icp()
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
            .args(["identity", "new", name])
            .assert()
            .success();
    }

    pub fn use_identity(&self, name: &str) {
        self.ctx
            .icp()
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
}
