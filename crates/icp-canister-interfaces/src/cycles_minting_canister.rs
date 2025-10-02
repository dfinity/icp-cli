use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

pub const CYCLES_MINTING_CANISTER_CID: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";
pub const CYCLES_MINTING_CANISTER_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 4, 1, 1]);

pub const MEMO_MINT_CYCLES: u64 = 0x544e494d; // == 'MINT'

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

pub type GetDefaultSubnetsRequest = ();
pub type GetDefaultSubnetsResponse = Vec<Principal>;

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
