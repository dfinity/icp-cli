use candid::{Decode, Encode, Nat, Principal};
use icrc_ledger_types::icrc1::account::{Account, Subaccount};
use pocket_ic::nonblocking::PocketIc;
use std::cell::Ref;

use crate::common::{CYCLES_LEDGER_CID, TestContext};

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
                Principal::from_text(CYCLES_LEDGER_CID).unwrap(),
                Principal::anonymous(),
                "icrc1_balance_of",
                bytes,
            )
            .await
            .unwrap();
        Decode!(result, Nat).unwrap()
    }
}
