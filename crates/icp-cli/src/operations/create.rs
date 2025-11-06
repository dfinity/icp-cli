use anyhow::anyhow;
use candid::{Decode, Encode, Principal};
use futures::future::join_all;
use ic_agent::Agent;
use icp::{
    context::{Context, EnvironmentSelection},
    identity::IdentitySelection,
    store_id::Key,
};
use icp_canister_interfaces::{
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    registry::{GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL},
};
use rand::seq::IndexedRandom;
use tokio::sync::OnceCell;

use crate::commands::canister::create::CanisterSettings;

pub(crate) struct CreateOperation<'a> {
    ctx: &'a Context,
    canisters: Vec<String>,
    environment: &'a EnvironmentSelection,
    identity: &'a IdentitySelection,
    subnet: Option<Principal>,
    controllers: Vec<Principal>,
    cycles: u128,
    settings: CanisterSettings,
    resolved_subnet: OnceCell<Result<Principal, String>>,
}

impl<'a> CreateOperation<'a> {
    pub(crate) fn new(
        ctx: &'a Context,
        canisters: Vec<String>,
        environment: &'a EnvironmentSelection,
        identity: &'a IdentitySelection,
        subnet: Option<Principal>,
        controllers: Vec<Principal>,
        cycles: u128,
        settings: CanisterSettings,
    ) -> Self {
        Self {
            ctx,
            canisters,
            environment,
            identity,
            subnet,
            controllers,
            cycles,
            settings,
            resolved_subnet: OnceCell::new(),
        }
    }

    /// 1. If a subnet is explicitly provided, use it
    /// 2. If no canisters exist yet, pick a random available subnet
    /// 3. If canisters exist, use the same subnet as the first existing canister
    ///
    /// Both successful results and errors are cached, so failed resolutions will not be retried.
    pub(crate) async fn get_subnet(&self, agent: &Agent) -> Result<Principal, String> {
        let result = self
            .resolved_subnet
            .get_or_init(|| async {
                // If subnet is explicitly provided, use it
                if let Some(subnet) = self.subnet {
                    return Ok(subnet);
                }

                // Get existing canisters from the environment
                let env = self
                    .ctx
                    .get_environment(self.environment)
                    .await
                    .map_err(|e| e.to_string())?;

                let existing_canisters: Vec<Principal> =
                    join_all(env.canisters.values().map(|(_, c)| async move {
                        self.ctx
                            .get_canister_id_for_env(&c.name, self.environment)
                            .await
                    }))
                    .await
                    .into_iter()
                    .filter_map(Result::ok)
                    .collect::<Vec<_>>();

                // If no canisters exist, pick a random available subnet
                if existing_canisters.is_empty() {
                    let subnets = match get_available_subnets(agent).await {
                        Ok(subnets) => subnets,
                        Err(e) => return Err(e.to_string()),
                    };

                    return subnets
                        .choose(&mut rand::rng())
                        .copied()
                        .ok_or_else(|| "no available subnets found".to_string());
                }

                // If canisters exist, use the same subnet as the first one
                get_canister_subnet(agent, &existing_canisters[0])
                    .await
                    .map_err(|e| e.to_string())
            })
            .await;

        result.clone()
    }
}

async fn get_canister_subnet(agent: &Agent, id: &Principal) -> Result<Principal, anyhow::Error> {
    let args = &GetSubnetForCanisterRequest {
        principal: Some(*id),
    };

    let bs = agent
        .query(&REGISTRY_PRINCIPAL, "get_subnet_for_canister")
        .with_arg(Encode!(args)?)
        .call()
        .await
        .map_err(|err| anyhow!("failed to get subnet: {}", err))?;

    let resp = Decode!(&bs, GetSubnetForCanisterResult)?;

    let out = resp
        .map_err(|err| anyhow!("failed to get subnet: {}", err))?
        .subnet_id
        .ok_or(anyhow!("missing subnet id"))?;

    Ok(out)
}

async fn get_available_subnets(agent: &Agent) -> Result<Vec<Principal>, anyhow::Error> {
    let bs = agent
        .query(&CYCLES_MINTING_CANISTER_PRINCIPAL, "get_default_subnets")
        .with_arg(Encode!(&())?)
        .call()
        .await
        .map_err(|err| anyhow!("failed to get available subnets: {}", err))?;

    let resp = Decode!(&bs, Vec<Principal>)?;

    // Check if any subnets are available
    if resp.is_empty() {
        return Err(anyhow!("no available subnets found"));
    }

    Ok(resp)
}
