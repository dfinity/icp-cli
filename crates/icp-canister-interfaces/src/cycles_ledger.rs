use bigdecimal::BigDecimal;
use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

/// 100m cycles
pub const CYCLES_LEDGER_BLOCK_FEE: u128 = 100_000_000;
pub const CYCLES_LEDGER_DECIMALS: i64 = 12;

pub const CYCLES_LEDGER_CID: &str = "um5iw-rqaaa-aaaaq-qaaba-cai";
pub const CYCLES_LEDGER_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 2, 1, 1]);

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct CanisterSettingsArg {
    pub freezing_threshold: Option<Nat>,
    pub controllers: Option<Vec<Principal>>,
    pub reserved_cycles_limit: Option<Nat>,
    pub memory_allocation: Option<Nat>,
    pub compute_allocation: Option<Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum SubnetSelectionArg {
    Filter { subnet_type: Option<String> },
    Subnet { subnet: Principal },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct CreationArgs {
    pub subnet_selection: Option<SubnetSelectionArg>,
    pub settings: Option<CanisterSettingsArg>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct CreateCanisterArgs {
    pub from_subaccount: Option<Vec<u8>>,
    pub created_at_time: Option<u64>,
    pub amount: Nat,
    pub creation_args: Option<CreationArgs>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum CreateCanisterResponse {
    Ok {
        block_id: Nat,
        canister_id: Principal,
    },
    Err(CreateCanisterError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum CreateCanisterError {
    GenericError {
        message: String,
        error_code: Nat,
    },
    TemporarilyUnavailable,
    Duplicate {
        duplicate_of: Nat,
        canister_id: Option<Principal>,
    },
    CreatedInFuture {
        ledger_time: u64,
    },
    FailedToCreate {
        error: String,
        refund_block: Option<Nat>,
        fee_block: Option<Nat>,
    },
    TooOld,
    InsufficientFunds {
        balance: Nat,
    },
}

impl CreateCanisterError {
    pub fn format_error(self, requested_cycles: u128) -> String {
        match self {
            CreateCanisterError::GenericError {
                message,
                error_code,
            } => {
                format!("Cycles ledger error (code {error_code}): {message}")
            }
            CreateCanisterError::TemporarilyUnavailable => {
                "Cycles ledger temporarily unavailable. Please retry in a moment.".to_string()
            }
            CreateCanisterError::Duplicate {
                duplicate_of,
                canister_id,
            } => {
                if let Some(canister_id) = canister_id {
                    format!(
                        "Duplicate request of block {duplicate_of}. Canister already created: {canister_id}"
                    )
                } else {
                    format!("Duplicate request of block {duplicate_of}.")
                }
            }
            CreateCanisterError::CreatedInFuture { .. } => {
                "created_at_time is too far in the future.".to_string()
            }
            CreateCanisterError::FailedToCreate {
                error,
                refund_block,
                fee_block,
            } => {
                let mut msg = format!("Failed to create canister: {error}");
                if let Some(b) = refund_block {
                    msg.push_str(&format!(". Refund block: {b}"));
                }
                if let Some(b) = fee_block {
                    msg.push_str(&format!(". Fee block: {b}"));
                }
                msg
            }
            CreateCanisterError::TooOld => "created_at_time is too old.".to_string(),
            CreateCanisterError::InsufficientFunds { balance } => {
                format!(
                    "Insufficient cycles. Requested: {} TCYCLES, available balance: {} TCYCLES. 
                    use `icp cycles mint` to get more cycles or use `--tcycles` to specify a different amount.",
                    BigDecimal::new(requested_cycles.into(), CYCLES_LEDGER_DECIMALS),
                    BigDecimal::from_biguint(balance.0.clone(), CYCLES_LEDGER_DECIMALS)
                )
            }
        }
    }
}

/// Returns a block index
pub type WithdrawOk = Nat;
pub type WithdrawResponse = Result<WithdrawOk, WithdrawError>;

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum RejectionCode {
    NoError,
    CanisterError,
    SysTransient,
    DestinationInvalid,
    Unknown,
    SysFatal,
    CanisterReject,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct WithdrawArgs {
    pub amount: Nat,
    pub from_subaccount: Option<Vec<u8>>,
    pub to: Principal,
    pub created_at_time: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum WithdrawError {
    GenericError {
        message: String,
        error_code: Nat,
    },
    TemporarilyUnavailable,
    FailedToWithdraw {
        fee_block: Option<Nat>,
        rejection_code: RejectionCode,
        rejection_reason: String,
    },
    Duplicate {
        duplicate_of: Nat,
    },
    BadFee {
        expected_fee: Nat,
    },
    InvalidReceiver {
        receiver: Principal,
    },
    CreatedInFuture {
        ledger_time: u64,
    },
    TooOld,
    InsufficientFunds {
        balance: Nat,
    },
}

impl WithdrawError {
    pub fn format_error(&self, requested_amount: u128) -> String {
        match self {
            WithdrawError::GenericError {
                message,
                error_code,
            } => {
                format!("Cycles ledger error (code {error_code}): {message}")
            }
            WithdrawError::TemporarilyUnavailable => {
                "Cycles ledger temporarily unavailable. Please retry in a moment.".to_string()
            }
            WithdrawError::FailedToWithdraw {
                rejection_code,
                rejection_reason,
                fee_block: _,
            } => {
                format!(
                    "Failed to withdraw cycles: {rejection_reason} (rejection code: {rejection_code:?})"
                )
            }
            WithdrawError::Duplicate { duplicate_of } => {
                format!("Duplicate request of block {duplicate_of}.")
            }
            WithdrawError::BadFee { expected_fee } => {
                format!("Bad fee. Expected fee: {expected_fee} cycles.")
            }
            WithdrawError::InvalidReceiver { receiver } => {
                format!("Invalid receiver: {receiver}")
            }
            WithdrawError::CreatedInFuture { .. } => {
                "created_at_time is too far in the future.".to_string()
            }
            WithdrawError::TooOld => "created_at_time is too old.".to_string(),
            WithdrawError::InsufficientFunds { balance } => {
                format!(
                    "Insufficient cycles. Requested: {}T cycles, balance: {}T cycles.",
                    BigDecimal::new(requested_amount.into(), CYCLES_LEDGER_DECIMALS),
                    BigDecimal::from_biguint(balance.0.clone(), CYCLES_LEDGER_DECIMALS)
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_ledger_cid_and_principal_match() {
        assert_eq!(CYCLES_LEDGER_CID, CYCLES_LEDGER_PRINCIPAL.to_text());
    }
}
