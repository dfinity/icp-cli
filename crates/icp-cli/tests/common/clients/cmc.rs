use candid::{Decode, Encode, Principal};
use icp_canister_interfaces::cycles_minting_canister::{
    CYCLES_MINTING_CANISTER_PRINCIPAL, GetDefaultSubnetsResponse,
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

    pub async fn get_default_subnets(&self) -> GetDefaultSubnetsResponse {
        let bytes = Encode!(&()).unwrap();
        let result = &self
            .pic
            .query_call(
                CYCLES_MINTING_CANISTER_PRINCIPAL,
                Principal::anonymous(),
                "get_default_subnets",
                bytes,
            )
            .await
            .unwrap();
        Decode!(result, GetDefaultSubnetsResponse).unwrap()
    }
}
