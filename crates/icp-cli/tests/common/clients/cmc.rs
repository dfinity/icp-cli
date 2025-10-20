use candid::{Decode, Encode, Principal};
use icp_canister_interfaces::cycles_minting_canister::{
    CYCLES_MINTING_CANISTER_PRINCIPAL, GetDefaultSubnetsResponse,
};
use pocket_ic::nonblocking::PocketIc;

use crate::common::TestContext;

pub(crate) struct Client<'a> {
    pic: &'a PocketIc,
}

impl<'a> Client<'a> {
    pub(crate) fn new(ctx: &'a TestContext) -> Self {
        Self {
            pic: ctx.pocketic(),
        }
    }

    pub(crate) async fn get_default_subnets(&self) -> Vec<Principal> {
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
