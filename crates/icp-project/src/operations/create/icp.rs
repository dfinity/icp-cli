use std::time::Duration;

use bigdecimal::{BigDecimal, ToPrimitive};
use candid::{Decode, Encode, IDLArgs, IDLValue, Principal};
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferError, TransferResult,
};
use ic_management_canister_types::CanisterSettings;
use icp_canister_interfaces::{
    cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_CID, CYCLES_MINTING_CANISTER_PRINCIPAL, MEMO_CREATE_CANISTER,
        NotifyCreateCanisterArg, NotifyCreateCanisterResponse, NotifyError, SubnetSelection,
    },
    icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL},
};
use snafu::{OptionExt, ResultExt};
use tokio::{select, time::sleep};
use tracing::info;

use super::{
    CandidDecodeSnafu, CandidEncodeSnafu, CreateOperation, CreateOperationError,
    IcpAmountOverflowSnafu, NotifyCreateFailedSnafu, NotifyCreateInterruptedSnafu,
    NotifyCreateTimeoutSnafu, TransferFailedSnafu, TransferIcpSnafu, shell_quote,
};
use crate::calls::Call;
use crate::parsers::to_token_unit_amount;
use crate::signal::stop_signal;

/// How long to keep retrying `notify_create_canister` before giving up.
const NOTIFY_RETRY_TIMEOUT: Duration = Duration::from_secs(60);
/// Delay between `notify_create_canister` retries.
const NOTIFY_RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// The outcome of a single `notify_create_canister` attempt.
enum NotifyStep {
    /// The canister was created.
    Created(Principal),
    /// A transient failure; worth retrying.
    Retry(String),
    /// A definitive failure (e.g. the ICP was refunded); retrying will not help.
    Terminal(String),
}

impl CreateOperation {
    /// Fund creation by converting ICP to cycles through the CMC.
    ///
    /// Transfers the ICP to the CMC's account (a subaccount derived from the
    /// caller) with the create-canister memo, then calls `notify_create_canister`.
    /// The CMC mints the cycles, picks the subnet, and creates the canister.
    pub(super) async fn create_cmc(
        &self,
        settings: &CanisterSettings,
        icp: &BigDecimal,
        recovery_flags: &str,
    ) -> Result<Principal, CreateOperationError> {
        let caller = self.inner.calls.caller();

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
        let transfer_args = TransferArgs {
            memo: Memo(MEMO_CREATE_CANISTER),
            amount: Tokens::from_e8s(e8s),
            fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
            from_subaccount: None,
            to,
            created_at_time: None,
        };
        let transfer_result = self
            .inner
            .calls
            .update(Call::new(
                ICP_LEDGER_PRINCIPAL,
                "transfer",
                Encode!(&transfer_args).context(CandidEncodeSnafu)?,
            ))
            .await
            .context(TransferIcpSnafu)?;
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
            .calls
            .update(Call::new(
                CYCLES_MINTING_CANISTER_PRINCIPAL,
                "notify_create_canister",
                arg_bytes.to_vec(),
            ))
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

#[cfg(test)]
mod tests {
    use candid::Nat;

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
}
