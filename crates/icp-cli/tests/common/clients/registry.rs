use candid::{Decode, Encode, Principal};
use ic_agent::Agent;
use icp_canister_interfaces::registry::{
    GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL,
};

use crate::common::TestContext;

pub(crate) struct Client {
    agent: Agent,
}

impl Client {
    pub(crate) fn new(ctx: &TestContext) -> Self {
        Self { agent: ctx.agent() }
    }

    pub(crate) async fn get_subnet_for_canister(&self, canister: Principal) -> Principal {
        let arg = GetSubnetForCanisterRequest {
            principal: Some(canister),
        };
        let bytes = Encode!(&arg).unwrap();
        let result = &self
            .agent
            .query(&REGISTRY_PRINCIPAL, "get_subnet_for_canister")
            .with_arg(bytes)
            .await
            .unwrap();
        Decode!(result, GetSubnetForCanisterResult)
            .expect("Failed to decode GetSubnetForCanisterResult")
            .expect("GetSubnetForCanisterResult returned an error")
            .subnet_id
            .expect("Canister not assigned to any subnet")
    }
}
