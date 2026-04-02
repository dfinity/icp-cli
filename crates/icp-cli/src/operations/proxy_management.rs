use candid::Principal;
use ic_agent::Agent;
use ic_management_canister_types::{CanisterIdRecord, CreateCanisterArgs};

use super::proxy::{UpdateOrProxyError, update_or_proxy};

/// Calls `create_canister` on the management canister, optionally routing
/// through a proxy canister.
pub async fn create_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    cycles: u128,
    args: CreateCanisterArgs,
) -> Result<CanisterIdRecord, UpdateOrProxyError> {
    let (result,): (CanisterIdRecord,) = update_or_proxy(
        agent,
        Principal::management_canister(),
        "create_canister",
        (args,),
        proxy,
        cycles,
    )
    .await?;

    Ok(result)
}
