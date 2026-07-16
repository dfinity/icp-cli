use candid::{CandidType, Nat, Principal};
use ic_management_canister_types::CanisterSettings;
use serde::Deserialize;

pub const CYCLES_MINTING_CANISTER_CID: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";
pub const CYCLES_MINTING_CANISTER_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 4, 1, 1]);

pub const MEMO_MINT_CYCLES: u64 = 0x544e494d; // == 'MINT'
pub const MEMO_CREATE_CANISTER: u64 = 0x41455243; // == 'CREA'

/// Response from get_icp_xdr_conversion_rate
#[derive(Debug, Deserialize, CandidType)]
pub struct ConversionRateResponse {
    pub data: ConversionRateData,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct ConversionRateData {
    pub xdr_permyriad_per_icp: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintArgs {
    pub block_index: u64,
    pub deposit_memo: Option<Vec<u8>>,
    pub to_subaccount: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintOk {
    pub balance: Nat,
    pub block_index: Nat,
    pub minted: Nat,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintRefunded {
    pub block_index: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintOther {
    pub error_message: String,
    pub error_code: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintErr {
    Refunded(NotifyMintRefunded),
    InvalidTransaction(String),
    Other(NotifyMintOther),
    Processing,
    TransactionTooOld(u64),
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintResponse {
    Ok(NotifyMintOk),
    Err(NotifyMintErr),
}

pub type GetDefaultSubnetsResponse = Vec<Principal>;

/// Selects which subnet the CMC should create a canister on.
#[derive(Debug, Clone, Deserialize, CandidType)]
pub enum SubnetSelection {
    /// Pick a random subnet matching the given filter.
    Filter { subnet_type: Option<String> },
    /// Create on a specific subnet.
    Subnet { subnet: Principal },
}

/// Argument to the CMC `notify_create_canister` method.
#[derive(Debug, Clone, Deserialize, CandidType)]
pub struct NotifyCreateCanisterArg {
    /// Block index of the ICP transfer that funds the creation.
    pub block_index: u64,
    /// Must be the caller. The CMC uses `settings` to set the real controllers.
    pub controller: Principal,
    pub subnet_selection: Option<SubnetSelection>,
    pub settings: Option<CanisterSettings>,
}

/// Error returned by the CMC `notify_*` methods.
#[derive(Debug, Clone, Deserialize, CandidType)]
pub enum NotifyError {
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    Processing,
    TransactionTooOld(u64),
    InvalidTransaction(String),
    Other {
        error_code: u64,
        error_message: String,
    },
}

impl NotifyError {
    pub fn format_error(&self) -> String {
        match self {
            NotifyError::Refunded {
                reason,
                block_index: Some(block_index),
            } => format!("Refunded at block {block_index}: {reason}"),
            NotifyError::Refunded {
                reason,
                block_index: None,
            } => format!("Refunded: {reason}"),
            NotifyError::Processing => {
                "The transaction is still being processed. Please retry in a moment.".to_string()
            }
            NotifyError::TransactionTooOld(block_index) => {
                format!("Transaction at block {block_index} is too old to be notified.")
            }
            NotifyError::InvalidTransaction(message) => {
                format!("Invalid transaction: {message}")
            }
            NotifyError::Other {
                error_code,
                error_message,
            } => format!("CMC error (code {error_code}): {error_message}"),
        }
    }
}

/// Response of the CMC `notify_create_canister` method.
pub type NotifyCreateCanisterResponse = Result<Principal, NotifyError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmc_cid_and_principal_match() {
        assert_eq!(
            CYCLES_MINTING_CANISTER_CID,
            CYCLES_MINTING_CANISTER_PRINCIPAL.to_text()
        );
    }
}
