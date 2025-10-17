use candid::{Decode, Encode, Nat, Principal};
use icp_canister_interfaces::{governance::GOVERNANCE_PRINCIPAL, icp_ledger::ICP_LEDGER_PRINCIPAL};
use icrc_ledger_types::icrc1::{
    account::{Account, Subaccount},
    transfer::TransferArg,
};
use pocket_ic::nonblocking::PocketIc;

use crate::common::TestContext;

pub struct Client<'a> {
    pic: &'a PocketIc,
}

impl<'a> Client<'a> {
    pub fn new(ctx: &'a TestContext) -> Self {
        Self {
            pic: ctx.pocketic(),
        }
    }

    pub async fn balance_of(&self, owner: Principal, subaccount: Option<Subaccount>) -> Nat {
        let arg = Account { owner, subaccount };
        let bytes = Encode!(&arg).unwrap();
        let result = &self
            .pic
            .query_call(
                ICP_LEDGER_PRINCIPAL,
                Principal::anonymous(),
                "icrc1_balance_of",
                bytes,
            )
            .await
            .unwrap();
        Decode!(result, Nat).unwrap()
    }

    pub async fn mint_icp(
        &self,
        owner: Principal,
        subaccount: Option<Subaccount>,
        amount: impl Into<Nat>,
    ) {
        let arg = TransferArg {
            from_subaccount: None,
            to: icrc_ledger_types::icrc1::account::Account { owner, subaccount },
            fee: None,
            created_at_time: None,
            memo: None,
            amount: amount.into(),
        };
        let bytes = Encode!(&arg).unwrap();
        self.pic
            .update_call(
                ICP_LEDGER_PRINCIPAL,
                GOVERNANCE_PRINCIPAL,
                "icrc1_transfer",
                bytes,
            )
            .await
            .unwrap();
    }
}
