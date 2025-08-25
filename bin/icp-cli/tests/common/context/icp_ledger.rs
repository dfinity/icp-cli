use candid::{Decode, Encode, Nat, Principal};
use icrc_ledger_types::icrc1::account::Subaccount;
use pocket_ic::PocketIc;
use std::cell::Ref;

const GOVERNANCE_ID: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 1, 1]); // rrkah-fqaaa-aaaaa-aaaaq-cai
const LEDGER_ID: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 2, 1, 1]); // ryjl3-tyaaa-aaaaa-aaaba-cai

pub struct IcpLedgerPocketIcClient<'a> {
    pub pic: Ref<'a, PocketIc>,
}

impl<'a> IcpLedgerPocketIcClient<'a> {
    pub fn balance_of(&self, owner: Principal, subaccount: Option<Subaccount>) -> Nat {
        Decode!(
            &self
                .pic
                .query_call(
                    LEDGER_ID,
                    Principal::anonymous(),
                    "icrc1_balance_of",
                    Encode!(&icrc_ledger_types::icrc1::account::Account { owner, subaccount })
                        .unwrap(),
                )
                .unwrap(),
            Nat
        )
        .unwrap()
    }

    pub fn mint_icp(
        &self,
        owner: Principal,
        subaccount: Option<Subaccount>,
        amount: impl Into<Nat>,
    ) {
        self.pic
            .update_call(
                LEDGER_ID,
                GOVERNANCE_ID,
                "icrc1_transfer",
                Encode!(&icrc_ledger_types::icrc1::transfer::TransferArg {
                    from_subaccount: None,
                    to: icrc_ledger_types::icrc1::account::Account { owner, subaccount },
                    fee: None,
                    created_at_time: None,
                    memo: None,
                    amount: amount.into(),
                })
                .unwrap(),
            )
            .unwrap();
    }
}
