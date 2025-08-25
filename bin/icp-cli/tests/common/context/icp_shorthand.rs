use candid::Principal;

use crate::common::TestContext;

pub struct IcpShorthand<'a> {
    ctx: &'a TestContext,
}

impl<'a> IcpShorthand<'a> {
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
        Principal::from_text(&stdout.trim()).unwrap()
    }

    pub fn use_new_random_identity(&self) {
        let random_name = format!("alice-{}", rand::random::<u64>());
        self.ctx
            .icp()
            .args(["identity", "new", &random_name])
            .assert()
            .success();
        self.ctx
            .icp()
            .args(["identity", "default", &random_name])
            .assert()
            .success();
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
}
