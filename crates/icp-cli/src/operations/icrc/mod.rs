use std::sync::Arc;

use candid::Principal;
use ic_agent::Agent;

mod icrc1_balance;
pub(crate) use icrc1_balance::*;

mod icrc1_decimals;
pub(crate) use icrc1_decimals::*;

mod icrc1_fee;
pub(crate) use icrc1_fee::*;

mod icrc1_symbol;
pub(crate) use icrc1_symbol::*;

#[allow(clippy::type_complexity)]
pub(crate) struct Initializers {
    pub(crate) icrc1_balance: Box<dyn Fn(&Agent, Principal) -> Arc<dyn Icrc1Balance>>,
    pub(crate) icrc1_decimals: Box<dyn Fn(&Agent, Principal) -> Arc<dyn Icrc1Decimals>>,
    pub(crate) icrc1_fee: Box<dyn Fn(&Agent, Principal) -> Arc<dyn Icrc1Fee>>,
    pub(crate) icrc1_symbol: Box<dyn Fn(&Agent, Principal) -> Arc<dyn Icrc1Symbol>>,
}

impl Default for Initializers {
    fn default() -> Self {
        Self {
            icrc1_balance: Box::new(|_, _| unimplemented!()),
            icrc1_decimals: Box::new(|_, _| unimplemented!()),
            icrc1_fee: Box::new(|_, _| unimplemented!()),
            icrc1_symbol: Box::new(|_, _| unimplemented!()),
        }
    }
}
