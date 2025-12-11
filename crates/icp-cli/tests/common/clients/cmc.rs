use candid::{Decode, Encode, Principal};
use ic_agent::Agent;
use icp_canister_interfaces::cycles_minting_canister::{
    CYCLES_MINTING_CANISTER_PRINCIPAL, GetDefaultSubnetsResponse,
};

use crate::common::TestContext;

pub(crate) struct Client {
    agent: Agent,
}

impl Client {
    pub(crate) fn new(ctx: &TestContext) -> Self {
        Self { agent: ctx.agent() }
    }

    pub(crate) async fn get_default_subnets(&self) -> Vec<Principal> {
        let bytes = Encode!(&()).unwrap();
        let result = &self
            .agent
            .query(&CYCLES_MINTING_CANISTER_PRINCIPAL, "get_default_subnets")
            .with_arg(bytes)
            .await
            .unwrap();
        Decode!(result, GetDefaultSubnetsResponse).unwrap()
    }
}
