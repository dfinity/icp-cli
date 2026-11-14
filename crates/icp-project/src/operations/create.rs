use std::sync::Arc;

use crate::calls::{Call, CanisterCalls, RouteTo};
use candid::{Decode, Encode, Nat, Principal};
use ic_management_canister_types::{
    CanisterIdRecord, CanisterSettings, CreateCanisterArgs as MgmtCreateCanisterArgs,
};
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CreateCanisterArgs, CreateCanisterResponse, CreationArgs,
        SubnetSelectionArg,
    },
    cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL,
    engine_canister::{
        GET_ENGINE_OPERATOR_BY_SUBNET_METHOD, GetEngineOperatorBySubnetArgs,
        GetEngineOperatorBySubnetResult,
    },
};
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::sync::OnceCell;
use tracing::warn;

use super::proxy_management;

/// Paying for a canister with ICP, which is the one thing here that speaks to
/// the ICP ledger. Behind a feature because the ledger's types bring a canister
/// SDK with them, and a caller that only ever pays in cycles — `deploy` — has
/// no use for either.
#[cfg(feature = "icp-funding")]
mod icp;

#[derive(Debug, Snafu)]
pub enum CreateOperationError {
    #[snafu(display("failed to encode candid"))]
    CandidEncode { source: candid::Error },

    #[snafu(display("failed to decode candid"))]
    CandidDecode { source: candid::Error },

    #[snafu(display("a canister call failed"))]
    Call {
        #[snafu(source(from(crate::calls::CallError, Box::new)))]
        source: Box<crate::calls::CallError>,
    },

    #[snafu(transparent)]
    TypedCall {
        source: crate::calls::TypedCallError,
    },

    #[snafu(display("failed to create canister: {message}"))]
    CreateCanister { message: String },

    #[snafu(display(
        "failed to check whether subnet {subnet} creates canisters through an engine operator"
    ))]
    CheckEngineOperator {
        #[snafu(source(from(crate::calls::CallError, Box::new)))]
        source: Box<crate::calls::CallError>,
        subnet: Principal,
    },

    #[snafu(display("failed to submit create_canister to subnet {subnet}"))]
    SubmitSubnetCreate {
        #[snafu(source(from(crate::calls::CallError, Box::new)))]
        source: Box<crate::calls::CallError>,
        subnet: Principal,
    },

    #[snafu(display("failed to query the engine-canister registry"))]
    EngineCanisterQuery {
        #[snafu(source(from(crate::calls::CallError, Box::new)))]
        source: Box<crate::calls::CallError>,
    },

    #[snafu(display(
        "could not resolve an engine-operator for CloudEngine subnet {subnet} via engine-canister {engine_registry} (no operator registered, or the registry is not deployed on this network)"
    ))]
    NoEngineOperator {
        subnet: Principal,
        engine_registry: Principal,
    },

    #[snafu(display("registry error: {message}"))]
    Registry { message: String },

    #[snafu(display("missing subnet id in registry response"))]
    MissingSubnetId,

    #[snafu(display("failed to get available subnets"))]
    GetAvailableSubnets {
        #[snafu(source(from(crate::calls::CallError, Box::new)))]
        source: Box<crate::calls::CallError>,
    },

    #[snafu(display("no available subnets found"))]
    NoAvailableSubnets,

    #[snafu(display("failed to resolve subnet: {message}"))]
    SubnetResolution { message: String },

    #[snafu(display("failed to get caller principal: {message}"))]
    GetPrincipal { message: String },

    #[cfg(feature = "icp-funding")]
    #[snafu(display("ICP amount is too large"))]
    IcpAmountOverflow,

    #[cfg(feature = "icp-funding")]
    #[snafu(display("invalid ICP amount: {message}"))]
    InvalidIcpAmount { message: String },

    #[cfg(feature = "icp-funding")]
    #[snafu(display("failed to transfer ICP to the cycles minting canister"))]
    TransferIcp {
        #[snafu(source(from(crate::calls::CallError, Box::new)))]
        source: Box<crate::calls::CallError>,
    },

    #[cfg(feature = "icp-funding")]
    #[snafu(display("ICP ledger transfer failed: {message}"))]
    TransferFailed { message: String },

    #[cfg(feature = "icp-funding")]
    #[snafu(display("failed to create canister via the cycles minting canister: {message}"))]
    NotifyCreateFailed { message: String },

    #[cfg(feature = "icp-funding")]
    #[snafu(display(
        "the cycles minting canister did not confirm creation within one minute.\n\
         Your ICP was transferred to the CMC at block {height}; no cycles were lost. \
         Once the CMC has caught up, complete the creation by running:\n\n    {command}\n"
    ))]
    NotifyCreateTimeout { height: u64, command: String },

    #[cfg(feature = "icp-funding")]
    #[snafu(display(
        "interrupted while waiting for the cycles minting canister to confirm creation.\n\
         Your ICP was transferred to the CMC at block {height}; no cycles were lost. \
         Complete the creation by running:\n\n    {command}\n"
    ))]
    NotifyCreateInterrupted { height: u64, command: String },
}

/// How canister creation is funded.
pub enum CreateFunding {
    /// Attach cycles from the cycles ledger (or provisional/proxy creation).
    Cycles(u128),
    /// Convert ICP to cycles through the cycles minting canister (CMC).
    #[cfg(feature = "icp-funding")]
    Icp {
        /// Amount of ICP to convert into cycles.
        amount: bigdecimal::BigDecimal,
        /// Identity/network/environment flags to append to the CMC recovery
        /// command, so a timed-out or interrupted creation can be finished by
        /// pasting the printed command verbatim.
        recovery_flags: String,
    },
}

/// Determines how a new canister is created.
pub enum CreateTarget {
    /// Create the canister on a specific subnet, chosen by the caller.
    Subnet(Principal),
    /// Create the canister through the caller's proxy canister, which pays for
    /// it and whose subnet the new canister lands on. Which proxy that is is a
    /// property of the caller — every call it makes is forwarded through the
    /// same one — so the target only says that the cycles come from there.
    Proxy,
    /// No explicit target. The subnet is resolved automatically: either from an
    /// existing canister in the project or by picking a random available subnet.
    None,
}

struct CreateOperationInner {
    calls: Arc<dyn CanisterCalls>,
    random: Arc<dyn crate::random::Random>,
    target: CreateTarget,
    funding: CreateFunding,
    existing_canisters: Vec<Principal>,
    /// The engine-canister registry to ask which engine-operator serves a
    /// subnet. Resolved by the caller, which is where an override of it — an
    /// environment variable — is something to read at all.
    engine_registry: Principal,
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
        calls: Arc<dyn CanisterCalls>,
        random: Arc<dyn crate::random::Random>,
        target: CreateTarget,
        funding: CreateFunding,
        existing_canisters: Vec<Principal>,
        engine_registry: Principal,
    ) -> Self {
        Self {
            inner: Arc::new(CreateOperationInner {
                calls,
                random,
                target,
                funding,
                existing_canisters,
                engine_registry,
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
        // Funding with ICP always goes through the CMC, which handles subnet
        // selection and payment itself.
        #[cfg(feature = "icp-funding")]
        if let CreateFunding::Icp {
            amount,
            recovery_flags,
        } = &self.inner.funding
        {
            return self.create_cmc(settings, amount, recovery_flags).await;
        }

        if let CreateTarget::Proxy = self.inner.target {
            return self.create_proxy(settings).await;
        }

        let selected_subnet = self
            .get_subnet()
            .await
            .map_err(|e| CreateOperationError::SubnetResolution { message: e })?;
        let uses_engine_operator = self
            .inner
            .calls
            .subnet_uses_engine_operator(selected_subnet)
            .await
            .context(CheckEngineOperatorSnafu {
                subnet: selected_subnet,
            })?;
        let cid = if uses_engine_operator {
            // Resolve the subnet's engine-operator first. Only a definitive
            // "could not resolve an operator" resolution failure falls back to
            // the legacy management-canister path — this covers both no operator
            // being registered for the subnet and the engine-canister registry
            // not being deployed on this network (a `DestinationInvalid` reject,
            // mapped to `NoEngineOperator` in `resolve_engine_operator`). Every
            // other resolution error (invalid registry id, other query/decode
            // failures) is propagated. Crucially, the fallback is decided
            // *before* any operator `create_canister` update is submitted: once
            // we hand off to the operator, a failure may mean the canister was
            // already created, so retrying via the management canister could
            // double-create and double-charge. Those errors propagate too.
            match self.resolve_engine_operator(selected_subnet).await {
                Ok(operator) => self.create_via_operator(settings, operator).await?,
                Err(e @ CreateOperationError::NoEngineOperator { .. }) => {
                    warn!(
                        "{e}; falling back to management-canister creation on this CloudEngine subnet"
                    );
                    self.create_mgmt(settings, selected_subnet).await?
                }
                Err(e) => return Err(e),
            }
        } else {
            self.create_ledger(settings, selected_subnet).await?
        };
        Ok(cid)
    }

    /// Cycles amount for the cycles-ledger and proxy paths. Panics if called on
    /// an ICP-funded operation, which never routes through those paths.
    fn cycles(&self) -> u128 {
        match self.inner.funding {
            CreateFunding::Cycles(cycles) => cycles,
            #[cfg(feature = "icp-funding")]
            CreateFunding::Icp { .. } => {
                panic!("cycles() called on an ICP-funded create operation")
            }
        }
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
            amount: Nat::from(self.cycles()),
            creation_args: Some(creation_args),
        };

        // Call cycles ledger create_canister
        let resp = self
            .inner
            .calls
            .update(Call::new(
                CYCLES_LEDGER_PRINCIPAL,
                "create_canister",
                Encode!(&arg).context(CandidEncodeSnafu)?,
            ))
            .await
            .context(CallSnafu)?;
        let resp: CreateCanisterResponse =
            Decode!(&resp, CreateCanisterResponse).context(CandidDecodeSnafu)?;
        let cid = match resp {
            CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
            CreateCanisterResponse::Err(err) => {
                return CreateCanisterSnafu {
                    message: err.format_error(self.cycles()),
                }
                .fail();
            }
        };
        Ok(cid)
    }

    /// Ask the engine-canister registry which engine-operator serves `subnet`.
    async fn resolve_engine_operator(
        &self,
        subnet: Principal,
    ) -> Result<Principal, CreateOperationError> {
        let engine_registry = self.inner.engine_registry;

        let arg = GetEngineOperatorBySubnetArgs {
            subnet_id: Some(subnet),
        };
        let resp = match self
            .inner
            .calls
            .query(Call::new(
                engine_registry,
                GET_ENGINE_OPERATOR_BY_SUBNET_METHOD,
                Encode!(&arg).context(CandidEncodeSnafu)?,
            ))
            .await
        {
            Ok(resp) => resp,
            // The engine-canister registry itself is not deployed on this
            // network (e.g. a local/test env). The replica rejects the query
            // with `DestinationInvalid` ("Canister ... not found", IC0301).
            // Treat a missing registry the same as "no operator registered":
            // this happens before any operator update is submitted, so the
            // legacy management-canister fallback is safe.
            Err(e) if e.is_canister_not_found() => {
                return NoEngineOperatorSnafu {
                    subnet,
                    engine_registry,
                }
                .fail();
            }
            Err(e) => return Err(e).context(EngineCanisterQuerySnafu),
        };
        let resp = Decode!(&resp, GetEngineOperatorBySubnetResult).context(CandidDecodeSnafu)?;

        resp.engine_operator_id.context(NoEngineOperatorSnafu {
            subnet,
            engine_registry,
        })
    }

    /// Delegate creation to a subnet's engine-operator canister. The operator's
    /// `create_canister` is byte-compatible with the cycles ledger, so we reuse
    /// the same request/response types and simply target the operator principal.
    async fn create_via_operator(
        &self,
        settings: &CanisterSettings,
        operator: Principal,
    ) -> Result<Principal, CreateOperationError> {
        let creation_args = CreationArgs {
            // CloudEngine subnets only create on themselves; the operator
            // ignores subnet selection, so leave it unset.
            subnet_selection: None,
            settings: Some(settings.clone()),
        };
        let arg = CreateCanisterArgs {
            from_subaccount: None,
            created_at_time: None,
            amount: Nat::from(self.cycles()),
            creation_args: Some(creation_args),
        };

        let resp = self
            .inner
            .calls
            .update(Call::new(
                operator,
                "create_canister",
                Encode!(&arg).context(CandidEncodeSnafu)?,
            ))
            .await
            .context(CallSnafu)?;
        let resp: CreateCanisterResponse =
            Decode!(&resp, CreateCanisterResponse).context(CandidDecodeSnafu)?;
        let cid = match resp {
            CreateCanisterResponse::Ok { canister_id, .. } => canister_id,
            CreateCanisterResponse::Err(err) => {
                return CreateCanisterSnafu {
                    message: err.format_error(self.cycles()),
                }
                .fail();
            }
        };
        Ok(cid)
    }

    /// Legacy CloudEngine creation: call the management canister's
    /// `create_canister`, routing to the target subnet by its id (subnet-scoped,
    /// so no canister on that subnet is needed to derive an effective canister
    /// id). Requires subnet-administrator permissions. Kept as a fallback for
    /// the engine-operator path.
    async fn create_mgmt(
        &self,
        settings: &CanisterSettings,
        subnet: Principal,
    ) -> Result<Principal, CreateOperationError> {
        let arg = encode_create_canister_arg(settings).context(CandidEncodeSnafu)?;

        // A subnet-scoped call is routed to the subnet rather than to any
        // canister on it; what that takes to submit and await is the caller
        // implementation's business.
        let bytes = self
            .inner
            .calls
            .update(
                Call::new(Principal::management_canister(), "create_canister", arg)
                    .with_route(RouteTo::Subnet(subnet)),
            )
            .await
            .context(SubmitSubnetCreateSnafu { subnet })?;
        let (record,): (CanisterIdRecord,) =
            candid::decode_args(&bytes).context(CandidDecodeSnafu)?;
        Ok(record.canister_id)
    }

    /// Creation paid for by the proxy the caller already routes through: an
    /// ordinary `create_canister` with cycles attached, which only a call made
    /// from inside a canister can carry.
    async fn create_proxy(
        &self,
        settings: &CanisterSettings,
    ) -> Result<Principal, CreateOperationError> {
        let args = MgmtCreateCanisterArgs {
            settings: Some(settings.clone()),
            sender_canister_version: None,
        };

        let result =
            proxy_management::create_canister(self.inner.calls.as_ref(), self.cycles(), args)
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
                    self.inner
                        .calls
                        .subnet_of(*canister)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    // If no canisters exist, pick a random available subnet
                    let subnets = get_available_subnets(self.inner.calls.as_ref())
                        .await
                        .map_err(|e| e.to_string())?;
                    if subnets.is_empty() {
                        return Err("no available subnets found".to_string());
                    }

                    let chosen = self
                        .inner
                        .random
                        .index_below(subnets.len())
                        .await
                        .map_err(|e| e.to_string())?;
                    // `index_below` promises an index below the count, and the
                    // count is not zero, so a miss is the seam's bug and says so
                    // rather than posing as an empty list.
                    chosen.and_then(|i| subnets.get(i).copied()).ok_or_else(|| {
                        format!(
                            "randomness chose {chosen:?}, which is not one of the {} \
                             available subnets",
                            subnets.len()
                        )
                    })
                }
            })
            .await;

        result.clone()
    }
}

/// Single-quotes a value for safe pasting into a POSIX shell, escaping embedded
/// single quotes with the `'\''` idiom.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Whether `err` is a rejection meaning the target canister does not exist on
/// this network (IC0301, "Canister ... not found"). Used to treat a missing
/// engine-canister registry as "no operator registered" so the CloudEngine path
/// can fall back safely.
///
async fn get_available_subnets(
    calls: &dyn CanisterCalls,
) -> Result<Vec<Principal>, CreateOperationError> {
    let bs = calls
        .query(Call::new(
            CYCLES_MINTING_CANISTER_PRINCIPAL,
            "get_default_subnets",
            Encode!(&()).context(CandidEncodeSnafu)?,
        ))
        .await
        .context(GetAvailableSubnetsSnafu)?;

    let resp = Decode!(&bs, Vec<Principal>).context(CandidDecodeSnafu)?;

    // Check if any subnets are available
    if resp.is_empty() {
        return NoAvailableSubnetsSnafu.fail();
    }

    Ok(resp)
}

/// Encodes the argument of the management canister's `create_canister`.
///
/// The interface defines it as `record { settings : opt canister_settings; ... }`,
/// so the settings have to be wrapped: a bare `canister_settings` decodes as "no
/// settings" and the canister is created with defaults.
fn encode_create_canister_arg(settings: &CanisterSettings) -> candid::Result<Vec<u8>> {
    Encode!(&MgmtCreateCanisterArgs {
        settings: Some(settings.clone()),
        sender_canister_version: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `create_canister` takes `record { settings : opt canister_settings; ... }`,
    /// not a bare `canister_settings`. Encoding the latter — which is what the
    /// `ic-utils` builder this path used to go through does — decodes as "no
    /// settings", and the canister is created with defaults.
    #[test]
    fn subnet_create_encodes_settings_inside_the_argument_record() {
        let settings = CanisterSettings {
            controllers: Some(vec![Principal::anonymous()]),
            ..Default::default()
        };

        let arg = encode_create_canister_arg(&settings).unwrap();
        let (decoded,): (MgmtCreateCanisterArgs,) = candid::decode_args(&arg).unwrap();
        assert_eq!(decoded.settings, Some(settings.clone()));

        let bare = Encode!(&settings).unwrap();
        let (as_args,): (MgmtCreateCanisterArgs,) = candid::decode_args(&bare).unwrap();
        assert!(as_args.settings.is_none());
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        // An embedded single quote is closed, escaped, and reopened via `'\''`.
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }
}
