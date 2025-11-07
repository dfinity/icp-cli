use anyhow::{Error, anyhow};
use candid::{Decode, Encode, Nat, Principal};
use futures::future::join_all;
use ic_agent::Agent;
use icp::{
    context::{Context, EnvironmentSelection},
    identity::IdentitySelection,
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CanisterSettingsArg, CreateCanisterArgs, CreateCanisterResponse,
        CreationArgs, SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    registry::{GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL},
};
use indicatif::ProgressBar;
use rand::seq::IndexedRandom;
use std::sync::Arc;
use tokio::sync::OnceCell;

use crate::commands::canister::create::CanisterSettings;

struct CreateOperationInner<'a> {
    ctx: &'a Context,
    environment: &'a EnvironmentSelection,
    identity: &'a IdentitySelection,
    subnet: Option<Principal>,
    controllers: Vec<Principal>,
    cycles: u128,
    settings: CanisterSettings,
    resolved_subnet: OnceCell<Result<Principal, String>>,
}

pub(crate) struct CreateOperation<'a> {
    inner: Arc<CreateOperationInner<'a>>,
}

impl<'a> Clone for CreateOperation<'a> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<'a> CreateOperation<'a> {
    pub(crate) fn new(
        ctx: &'a Context,
        environment: &'a EnvironmentSelection,
        identity: &'a IdentitySelection,
        subnet: Option<Principal>,
        controllers: Vec<Principal>,
        cycles: u128,
        settings: CanisterSettings,
    ) -> Self {
        Self {
            inner: Arc::new(CreateOperationInner {
                ctx,
                environment,
                identity,
                subnet,
                controllers,
                cycles,
                settings,
                resolved_subnet: OnceCell::new(),
            }),
        }
    }

    /// Creates the canister if it does not exist yet.
    /// Returns
    /// - `Ok((canister, None))` if the canister already exists.
    /// - `Ok((canister, Some(principal)))` if the canister was created.
    /// - `Err(String)` if an error occurred.
    pub(crate) async fn create(
        &self,
        canister: &str,
        pb: &ProgressBar,
    ) -> Result<(String, Option<Principal>), Error> {
        let env = self
            .inner
            .ctx
            .get_environment(self.inner.environment)
            .await?;
        let (_path, info) = env.get_canister_info(canister).map_err(|e| anyhow!(e))?;
        if self
            .inner
            .ctx
            .get_canister_id_for_env(canister, self.inner.environment)
            .await
            .is_ok()
        {
            return Ok((canister.to_string(), None));
        }
        pb.set_message("Creating...");

        let settings = CanisterSettingsArg {
            freezing_threshold: self
                .inner
                .settings
                .freezing_threshold
                .or(info.settings.freezing_threshold)
                .map(Nat::from),
            controllers: if self.inner.controllers.is_empty() {
                None
            } else {
                Some(self.inner.controllers.clone())
            },
            reserved_cycles_limit: self
                .inner
                .settings
                .reserved_cycles_limit
                .or(info.settings.reserved_cycles_limit)
                .map(Nat::from),
            memory_allocation: self
                .inner
                .settings
                .memory_allocation
                .or(info.settings.memory_allocation)
                .map(Nat::from),
            compute_allocation: self
                .inner
                .settings
                .compute_allocation
                .or(info.settings.compute_allocation)
                .map(Nat::from),
        };
        let creation_args = CreationArgs {
            subnet_selection: Some(SubnetSelectionArg::Subnet {
                subnet: self.get_subnet().await.map_err(|e| anyhow!(e))?,
            }),
            settings: Some(settings),
        };
        let arg = CreateCanisterArgs {
            from_subaccount: None,
            created_at_time: None,
            amount: Nat::from(self.inner.cycles),
            creation_args: Some(creation_args),
        };

        // Call cycles ledger create_canister
        let resp = self
            .inner
            .ctx
            .get_agent_for_env(self.inner.identity, self.inner.environment)
            .await?
            .update(&CYCLES_LEDGER_PRINCIPAL, "create_canister")
            .with_arg(Encode!(&arg)?)
            .call_and_wait()
            .await?;
        let resp: CreateCanisterResponse = Decode!(&resp, CreateCanisterResponse)?;
        let cid = match resp {
            CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
            CreateCanisterResponse::Err(err) => {
                return Err(anyhow!(err.format_error(self.inner.cycles)));
            }
        };

        self.inner
            .ctx
            .set_canister_id_for_env(canister, cid, self.inner.environment)
            .await?;

        Ok((canister.to_string(), Some(cid)))
    }

    /// 1. If a subnet is explicitly provided, use it
    /// 2. If no canisters exist yet, pick a random available subnet
    /// 3. If canisters exist, use the same subnet as the first existing canister
    ///
    /// Both successful results and errors are cached, so failed resolutions will not be retried.
    async fn get_subnet(&self) -> Result<Principal, String> {
        let result = self
            .inner
            .resolved_subnet
            .get_or_init(|| async {
                // If subnet is explicitly provided, use it
                if let Some(subnet) = self.inner.subnet {
                    return Ok(subnet);
                }

                // Get existing canisters from the environment
                let env = self
                    .inner
                    .ctx
                    .get_environment(self.inner.environment)
                    .await
                    .map_err(|e| e.to_string())?;

                let existing_canisters: Vec<Principal> =
                    join_all(env.canisters.values().map(|(_, c)| async move {
                        self.inner
                            .ctx
                            .get_canister_id_for_env(&c.name, self.inner.environment)
                            .await
                    }))
                    .await
                    .into_iter()
                    .filter_map(Result::ok)
                    .collect::<Vec<_>>();

                // If no canisters exist, pick a random available subnet
                let agent = self
                    .inner
                    .ctx
                    .get_agent_for_env(self.inner.identity, self.inner.environment)
                    .await
                    .map_err(|e| e.to_string())?;
                if existing_canisters.is_empty() {
                    let subnets = match get_available_subnets(&agent).await {
                        Ok(subnets) => subnets,
                        Err(e) => return Err(e.to_string()),
                    };

                    return subnets
                        .choose(&mut rand::rng())
                        .copied()
                        .ok_or_else(|| "no available subnets found".to_string());
                }

                // If canisters exist, use the same subnet as the first one
                get_canister_subnet(&agent, &existing_canisters[0])
                    .await
                    .map_err(|e| e.to_string())
            })
            .await;

        result.clone()
    }
}

async fn get_canister_subnet(
    agent: &Agent,
    canister: &Principal,
) -> Result<Principal, anyhow::Error> {
    let args = &GetSubnetForCanisterRequest {
        principal: Some(*canister),
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
