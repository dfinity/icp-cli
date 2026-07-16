use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bigdecimal::{BigDecimal, ToPrimitive};
use candid::{Decode, Encode, IDLArgs, IDLValue, Nat, Principal};
use ic_agent::{
    Agent, AgentError,
    agent::{Subnet, SubnetType},
};
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Timestamp, Tokens, TransferArgs, TransferError,
    TransferResult,
};
use ic_management_canister_types::{
    CanisterIdRecord, CanisterSettings, CreateCanisterArgs as MgmtCreateCanisterArgs,
};
use icp::parsers::to_token_unit_amount;
use icp::signal::stop_signal;
use icp_canister_interfaces::{
    cycles_ledger::{
        CYCLES_LEDGER_PRINCIPAL, CreateCanisterArgs, CreateCanisterResponse, CreationArgs,
        SubnetSelectionArg,
    },
    cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_CID, CYCLES_MINTING_CANISTER_PRINCIPAL, MEMO_CREATE_CANISTER,
        NotifyCreateCanisterArg, NotifyCreateCanisterResponse, NotifyError, SubnetSelection,
    },
    icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL},
};
use rand::seq::IndexedRandom;
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::{select, sync::OnceCell, time::sleep};
use tracing::{info, warn};

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

    #[snafu(display("failed to get caller principal: {message}"))]
    GetPrincipal { message: String },

    #[snafu(display("ICP amount is too large"))]
    IcpAmountOverflow,

    #[snafu(display("invalid ICP amount: {message}"))]
    InvalidIcpAmount { message: String },

    #[snafu(display("failed to transfer ICP to the cycles minting canister: {source}"))]
    TransferIcp { source: AgentError },

    #[snafu(display("ICP ledger transfer failed: {message}"))]
    TransferFailed { message: String },

    #[snafu(display("failed to create canister via the cycles minting canister: {message}"))]
    NotifyCreateFailed { message: String },

    #[snafu(display(
        "the cycles minting canister did not confirm creation within one minute.\n\
         Your ICP was transferred to the CMC at block {height}; no cycles were lost. \
         Once the CMC has caught up, complete the creation by running:\n\n    {command}\n"
    ))]
    NotifyCreateTimeout { height: u64, command: String },

    #[snafu(display(
        "interrupted while waiting for the cycles minting canister to confirm creation.\n\
         Your ICP was transferred to the CMC at block {height}; no cycles were lost. \
         Complete the creation by running:\n\n    {command}\n"
    ))]
    NotifyCreateInterrupted { height: u64, command: String },

    #[snafu(transparent)]
    UpdateOrProxyCall { source: UpdateOrProxyError },
}

/// How long to keep retrying `notify_create_canister` before giving up.
const NOTIFY_RETRY_TIMEOUT: Duration = Duration::from_secs(60);
/// Delay between `notify_create_canister` retries.
const NOTIFY_RETRY_INTERVAL: Duration = Duration::from_secs(2);
/// How many times to attempt the funding transfer before giving up.
const TRANSFER_MAX_ATTEMPTS: u32 = 3;

/// The outcome of a single `notify_create_canister` attempt.
enum NotifyStep {
    /// The canister was created.
    Created(Principal),
    /// A transient failure; worth retrying.
    Retry(String),
    /// A definitive failure (e.g. the ICP was refunded); retrying will not help.
    Terminal(String),
}

/// How canister creation is funded.
pub enum CreateFunding {
    /// Attach cycles from the cycles ledger (or provisional/proxy creation).
    Cycles(u128),
    /// Convert ICP to cycles through the cycles minting canister (CMC).
    Icp {
        /// Amount of ICP to convert into cycles.
        amount: BigDecimal,
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
    funding: CreateFunding,
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
        funding: CreateFunding,
        existing_canisters: Vec<Principal>,
    ) -> Self {
        Self {
            inner: Arc::new(CreateOperationInner {
                agent,
                target,
                funding,
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
        // Funding with ICP always goes through the CMC, which handles subnet
        // selection and payment itself.
        if let CreateFunding::Icp {
            amount,
            recovery_flags,
        } = &self.inner.funding
        {
            return self.create_cmc(settings, amount, recovery_flags).await;
        }

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

    /// Cycles amount for the cycles-ledger and proxy paths. Panics if called on
    /// an ICP-funded operation, which never routes through those paths.
    fn cycles(&self) -> u128 {
        match self.inner.funding {
            CreateFunding::Cycles(cycles) => cycles,
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
                    message: err.format_error(self.cycles()),
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

        let result =
            proxy_management::create_canister(&self.inner.agent, Some(proxy), self.cycles(), args)
                .await?;

        Ok(result.canister_id)
    }

    /// Fund creation by converting ICP to cycles through the CMC.
    ///
    /// Transfers the ICP to the CMC's account (a subaccount derived from the
    /// caller) with the create-canister memo, then calls `notify_create_canister`.
    /// The CMC mints the cycles, picks the subnet, and creates the canister.
    async fn create_cmc(
        &self,
        settings: &CanisterSettings,
        icp: &BigDecimal,
        recovery_flags: &str,
    ) -> Result<Principal, CreateOperationError> {
        let caller = self
            .inner
            .agent
            .get_principal()
            .map_err(|message| CreateOperationError::GetPrincipal { message })?;

        // ICP ledger amounts are denominated in e8s (10^-8 ICP). Reject any amount
        // with more precision than e8s can represent rather than silently
        // truncating it (which would still charge the ledger fee).
        let e8s = to_token_unit_amount(icp.clone(), 8)
            .map_err(|message| CreateOperationError::InvalidIcpAmount { message })?
            .to_u64()
            .context(IcpAmountOverflowSnafu)?;

        // The CMC creates on the resolved subnet, matching the cycles-ledger path.
        let selected_subnet = self
            .get_subnet()
            .await
            .map_err(|e| CreateOperationError::SubnetResolution { message: e })?;

        // Transfer the ICP to the CMC's account, which is a subaccount of the CMC
        // derived from the caller's principal.
        let to = AccountIdentifier::new(
            &CYCLES_MINTING_CANISTER_PRINCIPAL,
            &Subaccount::from(caller),
        );
        // Fix the creation time so a retried transfer (after a lost response) is
        // recognized as a duplicate by the ledger, which then returns the original
        // block index instead of moving the ICP a second time.
        let created_at_time = Timestamp {
            timestamp_nanos: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is before the Unix epoch")
                .as_nanos() as u64,
        };
        let transfer_args = TransferArgs {
            memo: Memo(MEMO_CREATE_CANISTER),
            amount: Tokens::from_e8s(e8s),
            fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
            from_subaccount: None,
            to,
            created_at_time: Some(created_at_time),
        };
        // Encode once: the argument (including the fixed timestamp) is identical
        // across retries, which is exactly what makes the transfer idempotent.
        let transfer_bytes = Encode!(&transfer_args).context(CandidEncodeSnafu)?;

        // Retry transient transport failures. Because created_at_time is fixed, a
        // retry after a lost response is deduplicated by the ledger (returning
        // TxDuplicate below) rather than transferring the ICP again.
        let mut attempt = 1;
        let transfer_result = loop {
            match self
                .inner
                .agent
                .update(&ICP_LEDGER_PRINCIPAL, "transfer")
                .with_arg(transfer_bytes.clone())
                .call_and_wait()
                .await
            {
                Ok(bytes) => break bytes,
                Err(err) if attempt < TRANSFER_MAX_ATTEMPTS => {
                    warn!(
                        "ICP transfer to the cycles minting canister failed \
                         (attempt {attempt}), retrying: {err}"
                    );
                    attempt += 1;
                    sleep(NOTIFY_RETRY_INTERVAL).await;
                }
                Err(err) => return Err(err).context(TransferIcpSnafu),
            }
        };
        let block_index =
            match Decode!(&transfer_result, TransferResult).context(CandidDecodeSnafu)? {
                Ok(block_index) => block_index,
                Err(TransferError::TxDuplicate { duplicate_of }) => duplicate_of,
                Err(err) => {
                    return TransferFailedSnafu {
                        message: format!("{err:?}"),
                    }
                    .fail();
                }
            };

        // Ask the CMC to mint cycles from the transferred ICP and create the
        // canister. `controller` must be the caller; the real controllers are
        // set through `settings`.
        let arg = NotifyCreateCanisterArg {
            block_index,
            controller: caller,
            subnet_selection: Some(SubnetSelection::Subnet {
                subnet: selected_subnet,
            }),
            settings: Some(settings.clone()),
        };
        // Encode once: the argument does not change between retries, and an
        // encoding failure is a bug rather than something to retry.
        let arg_bytes = Encode!(&arg).context(CandidEncodeSnafu)?;

        // The CMC often reports `Processing` for a while after the transfer, so
        // retry until it confirms, up to a one-minute budget. On timeout or
        // interruption we surface the transfer's block height and the command to
        // finish creation manually, so the paid-for ICP is never stranded.
        info!("Waiting for the cycles minting canister to create the canister...");
        let notify_loop = async {
            // Only log the CMC's status when it changes, so a normal wait (which
            // repeats `Processing`) does not flood the output.
            let mut last_message: Option<String> = None;
            loop {
                match self.notify_create(&arg_bytes).await? {
                    NotifyStep::Created(canister_id) => return Ok(canister_id),
                    NotifyStep::Terminal(message) => {
                        return NotifyCreateFailedSnafu { message }.fail();
                    }
                    NotifyStep::Retry(message) => {
                        if last_message.as_deref() != Some(message.as_str()) {
                            info!("cycles minting canister is not ready yet: {message}");
                            last_message = Some(message);
                        }
                        sleep(NOTIFY_RETRY_INTERVAL).await;
                    }
                }
            }
        };

        select! {
            result = notify_loop => result,
            _ = sleep(NOTIFY_RETRY_TIMEOUT) => NotifyCreateTimeoutSnafu {
                height: block_index,
                command: notify_recovery_command(&arg, recovery_flags),
            }
            .fail(),
            _ = stop_signal() => NotifyCreateInterruptedSnafu {
                height: block_index,
                command: notify_recovery_command(&arg, recovery_flags),
            }
            .fail(),
        }
    }

    /// Performs a single `notify_create_canister` attempt, classifying the result
    /// into [`NotifyStep`]. Agent/transport errors and the CMC's own transient
    /// states are retryable; a refund is terminal.
    async fn notify_create(&self, arg_bytes: &[u8]) -> Result<NotifyStep, CreateOperationError> {
        let resp = match self
            .inner
            .agent
            .update(&CYCLES_MINTING_CANISTER_PRINCIPAL, "notify_create_canister")
            .with_arg(arg_bytes.to_vec())
            .call_and_wait()
            .await
        {
            Ok(resp) => resp,
            Err(err) => return Ok(NotifyStep::Retry(err.to_string())),
        };

        let resp = Decode!(&resp, NotifyCreateCanisterResponse).context(CandidDecodeSnafu)?;
        Ok(match resp {
            Ok(canister_id) => NotifyStep::Created(canister_id),
            // These are definitive outcomes for this block: the ICP was refunded,
            // or the transfer can no longer be notified. Re-notifying will never
            // succeed, so fail fast instead of retrying for a minute.
            Err(
                err @ (NotifyError::Refunded { .. }
                | NotifyError::TransactionTooOld(_)
                | NotifyError::InvalidTransaction(_)),
            ) => NotifyStep::Terminal(err.format_error()),
            // `Processing` is expected while the CMC works; `Other` may be a
            // transient internal error. Both are worth retrying.
            Err(err) => NotifyStep::Retry(err.format_error()),
        })
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

/// Builds the `icp canister call` command that re-runs `notify_create_canister`
/// for an already-paid transfer, so the user can finish a creation that timed out
/// or was interrupted.
///
/// `recovery_flags` carries the identity/network/environment selection used for
/// the original call, so the printed command targets the same network and identity
/// and can be pasted verbatim.
///
/// The argument is rendered from the exact typed `arg`, so every requested setting
/// is preserved and the manual call matches the original request.
fn notify_recovery_command(arg: &NotifyCreateCanisterArg, recovery_flags: &str) -> String {
    // Rendering a value we just constructed should never fail; fall back to the
    // essential fields if candid's textual conversion ever does.
    let rendered = IDLValue::try_from_candid_type(arg)
        .map(|value| IDLArgs::new(&[value]).to_string())
        .unwrap_or_else(|_| {
            format!(
                "(record {{ block_index = {} : nat64; controller = principal \"{}\" }})",
                arg.block_index, arg.controller
            )
        });
    // Single-quote the candid argument (and any selection flags) for the shell so
    // the command is safe to paste as-is.
    format!(
        "icp canister call {CYCLES_MINTING_CANISTER_CID} notify_create_canister {}{recovery_flags}",
        shell_quote(&rendered)
    )
}

/// Single-quotes a value for safe pasting into a POSIX shell, escaping embedded
/// single quotes with the `'\''` idiom.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_command_preserves_all_settings() {
        let arg = NotifyCreateCanisterArg {
            block_index: 42,
            controller: Principal::anonymous(),
            subnet_selection: Some(SubnetSelection::Subnet {
                subnet: CYCLES_MINTING_CANISTER_PRINCIPAL,
            }),
            settings: Some(CanisterSettings {
                controllers: Some(vec![Principal::anonymous()]),
                compute_allocation: Some(Nat::from(5u8)),
                memory_allocation: Some(Nat::from(4_294_967_296u64)),
                freezing_threshold: Some(Nat::from(2_592_000u64)),
                reserved_cycles_limit: Some(Nat::from(1_000_000_000u64)),
                ..Default::default()
            }),
        };

        let command = notify_recovery_command(&arg, " --identity alice --network ic");

        // The command targets the CMC's notify method with named candid fields, and
        // every requested setting survives the round-trip (not just controllers).
        assert!(command.contains("notify_create_canister"));
        assert!(command.contains("block_index = 42"));
        assert!(command.contains("compute_allocation = opt (5"));
        assert!(command.contains("memory_allocation = opt (4_294_967_296"));
        assert!(command.contains("freezing_threshold = opt (2_592_000"));
        assert!(command.contains("reserved_cycles_limit = opt (1_000_000_000"));

        // The identity/network selection is appended so the printed command targets
        // the same network and identity as the original call.
        assert!(
            command
                .trim_end()
                .ends_with(" --identity alice --network ic")
        );
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        // An embedded single quote is closed, escaped, and reopened via `'\''`.
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }
}
