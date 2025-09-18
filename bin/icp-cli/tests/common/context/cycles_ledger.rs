use candid::{Decode, Encode, Nat, Principal};
use icrc_ledger_types::icrc1::account::Subaccount;
use pocket_ic::nonblocking::PocketIc;
use std::cell::Ref;

const CYCLES_LEDGER_ID: Principal = Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 2, 1, 1]); // um5iw-rqaaa-aaaaq-qaaba-cai

pub struct CyclesLedgerPocketIcClient<'a> {
    pub pic: Ref<'a, PocketIc>,
}

impl CyclesLedgerPocketIcClient<'_> {
    pub async fn balance_of(&self, owner: Principal, subaccount: Option<Subaccount>) -> Nat {
        Decode!(
            &self
                .pic
                .query_call(
                    CYCLES_LEDGER_ID,
                    Principal::anonymous(),
                    "icrc1_balance_of",
                    Encode!(&icrc_ledger_types::icrc1::account::Account { owner, subaccount })
                        .unwrap(),
                )
                .await
                .unwrap(),
            Nat
        )
        .unwrap()
    }
}
