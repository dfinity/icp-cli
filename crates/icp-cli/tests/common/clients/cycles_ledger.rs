use candid::{Decode, Encode, Nat, Principal};
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icrc_ledger_types::icrc1::account::{Account, Subaccount};
use pocket_ic::nonblocking::PocketIc;
use std::cell::Ref;

use crate::common::TestContext;

pub struct CyclesLedgerPocketIcClient<'a> {
    pic: Ref<'a, PocketIc>,
}

impl<'a> CyclesLedgerPocketIcClient<'a> {
    pub fn new(ctx: &'a TestContext) -> Self {
        Self {
            pic: ctx.pocketic(),
        }
    }

    pub async fn balance_of(&self, owner: Principal, subaccount: Option<Subaccount>) -> Nat {
        let args = Account { owner, subaccount };
        let bytes = Encode!(&args).unwrap();
        let result = &self
            .pic
            .query_call(
                CYCLES_LEDGER_PRINCIPAL,
                Principal::anonymous(),
                "icrc1_balance_of",
                bytes,
            )
            .await
            .unwrap();
        Decode!(result, Nat).unwrap()
    }
}
