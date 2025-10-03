use candid::{Decode, Encode, Principal};
use icp_canister_interfaces::registry::{
    GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL,
};
use pocket_ic::nonblocking::PocketIc;
use std::cell::Ref;

use crate::common::TestContext;

pub struct Client<'a> {
    pic: Ref<'a, PocketIc>,
}

impl<'a> Client<'a> {
    pub fn new(ctx: &'a TestContext) -> Self {
        Self {
            pic: ctx.pocketic(),
        }
    }

    pub async fn get_subnet_for_canister(&self, canister: Principal) -> Principal {
        let arg = GetSubnetForCanisterRequest {
            principal: Some(canister),
        };
        let bytes = Encode!(&arg).unwrap();
        let result = &self
            .pic
            .query_call(
                REGISTRY_PRINCIPAL,
                Principal::anonymous(),
                "get_subnet_for_canister",
                bytes,
            )
            .await
            .unwrap();
        Decode!(result, GetSubnetForCanisterResult)
            .expect("Failed to decode GetSubnetForCanisterResult")
            .expect("GetSubnetForCanisterResult returned an error")
            .subnet_id
            .expect("Canister not assigned to any subnet")
    }
}
