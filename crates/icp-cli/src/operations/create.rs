use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{Agent, AgentError};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CanisterSettingsArg, CreateCanisterArgs, CreateCanisterResponse,
        CreationArgs, SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    registry::{GetSubnetForCanisterRequest, GetSubnetForCanisterResult, REGISTRY_PRINCIPAL},
};
use rand::seq::IndexedRandom;
use snafu::{ResultExt, Snafu};
use std::sync::Arc;
use tokio::sync::OnceCell;

#[derive(Debug, Snafu)]
pub enum CreateOperationError {
    #[snafu(display("failed to encode candid: {source}"))]
    CandidEncode { source: candid::Error },

    #[snafu(display("failed to decode candid: {source}"))]
    CandidDecode { source: candid::Error },

    #[snafu(display("agent error: {source}"))]
    Agent { source: AgentError },

    #[snafu(display("failed to create canister: {message}"))]
    CreateCanister { message: String },

    #[snafu(display("failed to get subnet for canister: {source}"))]
    GetSubnet { source: AgentError },

    #[snafu(display("registry error: {message}"))]
    Registry { message: String },

    #[snafu(display("missing subnet id in registry response"))]
    MissingSubnetId,

    #[snafu(display("failed to get available subnets: {source}"))]
    GetAvailableSubnets { source: AgentError },

    #[snafu(display("no available subnets found"))]
    NoAvailableSubnets,

    #[snafu(display("failed to resolve subnet: {message}"))]
    SubnetResolution { message: String },
}

struct CreateOperationInner {
    agent: Agent,
    subnet: Option<Principal>,
    cycles: u128,
    existing_canisters: Vec<Principal>,
    resolved_subnet: OnceCell<Result<Principal, String>>,
}

pub struct CreateOperation {
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
    pub fn new(
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
    /// - `Err(CreateOperationError)` if an error occurred.
    pub async fn create(
        &self,
        settings: &CanisterSettingsArg,
    ) -> Result<Principal, CreateOperationError> {
        let creation_args = CreationArgs {
            subnet_selection: Some(SubnetSelectionArg::Subnet {
                subnet: self
                    .get_subnet()
                    .await
                    .map_err(|e| CreateOperationError::SubnetResolution { message: e })?,
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
            .with_arg(Encode!(&arg).context(CandidEncodeSnafu)?)
            .call_and_wait()
            .await
            .context(AgentSnafu)?;
        let resp: CreateCanisterResponse =
            Decode!(&resp, CreateCanisterResponse).context(CandidDecodeSnafu)?;
        let cid = match resp {
            CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
            CreateCanisterResponse::Err(err) => {
                return CreateCanisterSnafu {
                    message: err.format_error(self.inner.cycles),
                }.fail();
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

                if let Some(canister) = self.inner.existing_canisters.first() {
                    let subnet = get_canister_subnet(&self.inner.agent, *canister)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(subnet)
                } else {
                    // If no canisters exist, pick a random available subnet
                    let subnets = get_available_subnets(&self.inner.agent)
                        .await
                        .map_err(|e| e.to_string())?;

                    subnets
                        .choose(&mut rand::rng())
                        .copied()
                        .ok_or_else(|| "no available subnets found".to_string())
                }
            })
            .await;

        result.clone()
    }
}

async fn get_canister_subnet(
    agent: &Agent,
    canister: Principal,
) -> Result<Principal, CreateOperationError> {
    let args = &GetSubnetForCanisterRequest {
        principal: Some(canister),
    };

    let bs = agent
        .query(&REGISTRY_PRINCIPAL, "get_subnet_for_canister")
        .with_arg(Encode!(args).context(CandidEncodeSnafu)?)
        .call()
        .await
        .context(GetSubnetSnafu)?;

    let resp = Decode!(&bs, GetSubnetForCanisterResult).context(CandidDecodeSnafu)?;

    let out = resp
        .map_err(|err| CreateOperationError::Registry { message: err })?
        .subnet_id
        .ok_or(CreateOperationError::MissingSubnetId)?;

    Ok(out)
}

async fn get_available_subnets(agent: &Agent) -> Result<Vec<Principal>, CreateOperationError> {
    let bs = agent
        .query(&CYCLES_MINTING_CANISTER_PRINCIPAL, "get_default_subnets")
        .with_arg(Encode!(&()).context(CandidEncodeSnafu)?)
        .call()
        .await
        .context(GetAvailableSubnetsSnafu)?;

    let resp = Decode!(&bs, Vec<Principal>).context(CandidDecodeSnafu)?;

    // Check if any subnets are available
    if resp.is_empty() {
        return NoAvailableSubnetsSnafu.fail();
    }

    Ok(resp)
}
