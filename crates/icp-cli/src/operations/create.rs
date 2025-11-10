use anyhow::{Error, anyhow};
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;
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

struct CreateOperationInner {
    agent: Agent,
    subnet: Option<Principal>,
    cycles: u128,
    existing_canisters: Vec<Principal>,
    resolved_subnet: OnceCell<Result<Principal, String>>,
}

pub(crate) struct CreateOperation {
    inner: Arc<CreateOperationInner>,
}

impl Clone for CreateOperation {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl CreateOperation {
    pub(crate) fn new(
        agent: Agent,
        subnet: Option<Principal>,
        cycles: u128,
        existing_canisters: Vec<Principal>,
    ) -> Self {
        Self {
            inner: Arc::new(CreateOperationInner {
                agent,
                subnet,
                cycles,
                existing_canisters,
                resolved_subnet: OnceCell::new(),
            }),
        }
    }

    /// Creates the canister if it does not exist yet.
    /// Returns
    /// - `Ok(principal)` if a canister was created.
    /// - `Err(String)` if an error occurred.
    pub(crate) async fn create(
        &self,
        settings: &CanisterSettingsArg,
        pb: &ProgressBar,
    ) -> Result<Principal, Error> {
        pb.set_message("Creating...");
        let creation_args = CreationArgs {
            subnet_selection: Some(SubnetSelectionArg::Subnet {
                subnet: self.get_subnet().await.map_err(|e| anyhow!(e))?,
            }),
            settings: Some(settings.clone()),
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
            .agent
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

        Ok(cid)
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

                if let Some(canister) = self.inner.existing_canisters.iter().next() {
                    let subnet = get_canister_subnet(&self.inner.agent, *canister)
                        .await
                        .map_err(|e| e.to_string())?;
                    return Ok(subnet);
                } else {
                    // If no canisters exist, pick a random available subnet
                    let subnets = match get_available_subnets(&self.inner.agent).await {
                        Ok(subnets) => subnets,
                        Err(e) => return Err(e.to_string()),
                    };

                    return subnets
                        .choose(&mut rand::rng())
                        .copied()
                        .ok_or_else(|| "no available subnets found".to_string());
                }
            })
            .await;

        result.clone()
    }
}

async fn get_canister_subnet(agent: &Agent, canister: Principal) -> Result<Principal, Error> {
    let args = &GetSubnetForCanisterRequest {
        principal: Some(canister),
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

async fn get_available_subnets(agent: &Agent) -> Result<Vec<Principal>, Error> {
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
