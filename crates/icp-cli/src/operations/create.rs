use std::sync::Arc;

use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{
    Agent, AgentError,
    agent::{Subnet, SubnetType},
};
use ic_management_canister_types::{
    CanisterIdRecord, CanisterSettings, CreateCanisterArgs as MgmtCreateCanisterArgs,
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CreateCanisterArgs, CreateCanisterResponse, CreationArgs,
        SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
};
use rand::seq::IndexedRandom;
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::sync::OnceCell;

use super::proxy::UpdateOrProxyError;
use super::proxy_management;

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

    #[snafu(transparent)]
    UpdateOrProxyCall { source: UpdateOrProxyError },
}

/// Determines how a new canister is created.
pub enum CreateTarget {
    /// Create the canister on a specific subnet, chosen by the caller.
    Subnet(Principal),
    /// Create the canister via a proxy canister. The `create_canister` call is
    /// forwarded through the proxy's `proxy` method to the management canister,
    /// so the new canister will be placed on the same subnet as the proxy.
    Proxy(Principal),
    /// No explicit target. The subnet is resolved automatically: either from an
    /// existing canister in the project or by picking a random available subnet.
    None,
}

struct CreateOperationInner {
    agent: Agent,
    target: CreateTarget,
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
        target: CreateTarget,
        cycles: u128,
        existing_canisters: Vec<Principal>,
    ) -> Self {
        Self {
            inner: Arc::new(CreateOperationInner {
                agent,
                target,
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
        settings: &CanisterSettings,
    ) -> Result<Principal, CreateOperationError> {
        if let CreateTarget::Proxy(proxy) = self.inner.target {
            return self.create_proxy(settings, proxy).await;
        }

        let selected_subnet = self
            .get_subnet()
            .await
            .map_err(|e| CreateOperationError::SubnetResolution { message: e })?;
        let subnet_info = self
            .inner
            .agent
            .get_subnet_by_id(&selected_subnet)
            .await
            .context(GetSubnetSnafu)?;
        let cid = if let Some(SubnetType::CloudEngine) = subnet_info.subnet_type() {
            self.create_mgmt(settings, &subnet_info).await?
        } else {
            self.create_ledger(settings, selected_subnet).await?
        };
        Ok(cid)
    }

    async fn create_ledger(
        &self,
        settings: &CanisterSettings,
        selected_subnet: Principal,
    ) -> Result<Principal, CreateOperationError> {
        let creation_args = CreationArgs {
            subnet_selection: Some(SubnetSelectionArg::Subnet {
                subnet: selected_subnet,
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
                }
                .fail();
            }
        };
        Ok(cid)
    }

    async fn create_mgmt(
        &self,
        settings: &CanisterSettings,
        selected_subnet: &Subnet,
    ) -> Result<Principal, CreateOperationError> {
        let arg = MgmtCreateCanisterArgs {
            settings: Some(settings.clone()),
            sender_canister_version: None,
        };

        // Call management canister create_canister
        let resp = self
            .inner
            .agent
            .update(&Principal::management_canister(), "create_canister")
            .with_arg(Encode!(&arg).context(CandidEncodeSnafu)?)
            .with_effective_canister_id(
                *selected_subnet
                    .iter_canister_ranges()
                    .next()
                    .context(CreateCanisterSnafu {
                        message: "subnet did not contain canister ranges",
                    })?
                    .start(),
            )
            .await
            .context(AgentSnafu)?;
        let resp: CanisterIdRecord = Decode!(&resp, CanisterIdRecord).context(CandidDecodeSnafu)?;
        Ok(resp.canister_id)
    }

    async fn create_proxy(
        &self,
        settings: &CanisterSettings,
        proxy: Principal,
    ) -> Result<Principal, CreateOperationError> {
        let args = MgmtCreateCanisterArgs {
            settings: Some(settings.clone()),
            sender_canister_version: None,
        };

        let result = proxy_management::create_canister(
            &self.inner.agent,
            Some(proxy),
            self.inner.cycles,
            args,
        )
        .await?;

        Ok(result.canister_id)
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
                if let CreateTarget::Subnet(subnet) = self.inner.target {
                    return Ok(subnet);
                }

                if let Some(canister) = self.inner.existing_canisters.first() {
                    let subnet = &self
                        .inner
                        .agent
                        .get_subnet_by_canister(canister)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(subnet.id())
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
