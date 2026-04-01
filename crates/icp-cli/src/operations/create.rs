use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{
    Agent, AgentError,
    agent::{Subnet, SubnetType},
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CreateCanisterArgs, CreateCanisterResponse, CreationArgs,
        SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    management_canister::{
        CanisterSettingsArg, MgmtCreateCanisterArgs, MgmtCreateCanisterResponse,
    },
    proxy::{ProxyArgs, ProxyResult},
};
use rand::seq::IndexedRandom;
use snafu::{OptionExt, ResultExt, Snafu};
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

    #[snafu(display("proxy call failed: {message}"))]
    ProxyCall { message: String },

    #[snafu(display("failed to decode proxy canister response: {source}"))]
    ProxyDecode { source: candid::Error },
}

struct CreateOperationInner {
    agent: Agent,
    proxy: Option<Principal>,
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
        proxy: Option<Principal>,
        subnet: Option<Principal>,
        cycles: u128,
        existing_canisters: Vec<Principal>,
    ) -> Self {
        Self {
            inner: Arc::new(CreateOperationInner {
                agent,
                proxy,
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
        if let Some(proxy) = self.inner.proxy {
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
        let cid = if let Some(SubnetType::Unknown(kind)) = subnet_info.subnet_type()
            && kind == "cloud_engine"
        {
            self.create_mgmt(settings, &subnet_info).await?
        } else {
            self.create_ledger(settings, selected_subnet).await?
        };
        Ok(cid)
    }

    async fn create_ledger(
        &self,
        settings: &CanisterSettingsArg,
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
        settings: &CanisterSettingsArg,
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
        let resp = Decode!(&resp, MgmtCreateCanisterResponse).context(CandidDecodeSnafu)?;
        Ok(resp.canister_id)
    }

    async fn create_proxy(
        &self,
        settings: &CanisterSettingsArg,
        proxy: Principal,
    ) -> Result<Principal, CreateOperationError> {
        let mgmt_arg = MgmtCreateCanisterArgs {
            settings: Some(settings.clone()),
            sender_canister_version: None,
        };
        let mgmt_arg_bytes = Encode!(&mgmt_arg).context(CandidEncodeSnafu)?;

        let proxy_args = ProxyArgs {
            canister_id: Principal::management_canister(),
            method: "create_canister".to_string(),
            args: mgmt_arg_bytes,
            cycles: Nat::from(self.inner.cycles),
        };
        let proxy_arg_bytes = Encode!(&proxy_args).context(CandidEncodeSnafu)?;

        let proxy_res = self
            .inner
            .agent
            .update(&proxy, "proxy")
            .with_arg(proxy_arg_bytes)
            .await
            .context(AgentSnafu)?;

        let proxy_result: (ProxyResult,) =
            candid::decode_args(&proxy_res).context(ProxyDecodeSnafu)?;

        match proxy_result.0 {
            ProxyResult::Ok(ok) => {
                let resp =
                    Decode!(&ok.result, MgmtCreateCanisterResponse).context(CandidDecodeSnafu)?;
                Ok(resp.canister_id)
            }
            ProxyResult::Err(err) => ProxyCallSnafu {
                message: err.format_error(),
            }
            .fail(),
        }
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
