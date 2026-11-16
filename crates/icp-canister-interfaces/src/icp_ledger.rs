use candid::{CandidType, Principal};
use serde::Deserialize;
use sha2::{Digest, Sha224};

/// 0.0001 ICP, a.k.a. 10k e8s
pub const ICP_LEDGER_BLOCK_FEE_E8S: u64 = 10_000;
pub const ICP_LEDGER_SYMBOL: &str = "ICP";
pub const ICP_LEDGER_CID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const ICP_LEDGER_PRINCIPAL: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 2, 1, 1]);

/// An arbitrary 32-byte array that, with a principal, names one ledger account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, CandidType)]
pub struct Subaccount(pub [u8; 32]);

impl From<Principal> for Subaccount {
    /// The subaccount the CMC credits a principal's deposits to: the
    /// principal's length, then its bytes.
    fn from(principal: Principal) -> Self {
        let principal = principal.as_slice();
        let mut subaccount = [0; 32];
        subaccount[0] = principal.len() as u8;
        subaccount[1..1 + principal.len()].copy_from_slice(principal);
        Subaccount(subaccount)
    }
}

/// An ICP ledger account identifier: a big-endian CRC32 of a SHA-224 hash,
/// followed by that hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, CandidType)]
pub struct AccountIdentifier([u8; 32]);

impl AccountIdentifier {
    pub fn new(owner: &Principal, subaccount: &Subaccount) -> Self {
        let mut hasher = Sha224::new();
        hasher.update(b"\x0Aaccount-id");
        hasher.update(owner.as_slice());
        hasher.update(subaccount.0);
        let hash = hasher.finalize();

        let mut result = [0u8; 32];
        result[0..4].copy_from_slice(&crc32fast::hash(&hash).to_be_bytes());
        result[4..32].copy_from_slice(&hash);
        Self(result)
    }
}

impl AsRef<[u8]> for AccountIdentifier {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, CandidType)]
pub struct Tokens {
    pub e8s: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, CandidType)]
pub struct Memo(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, CandidType)]
pub struct Timestamp {
    pub timestamp_nanos: u64,
}

/// Arguments for the ICP ledger's `transfer` method.
#[derive(Debug, Clone, Deserialize, CandidType)]
pub struct TransferArgs {
    pub memo: Memo,
    pub amount: Tokens,
    pub fee: Tokens,
    pub from_subaccount: Option<Subaccount>,
    pub to: AccountIdentifier,
    pub created_at_time: Option<Timestamp>,
}

/// Result of the ICP ledger's `transfer` method: the block index on success.
pub type TransferResult = Result<u64, TransferError>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, CandidType)]
pub enum TransferError {
    BadFee { expected_fee: Tokens },
    InsufficientFunds { balance: Tokens },
    TxTooOld { allowed_window_nanos: u64 },
    TxCreatedInFuture,
    TxDuplicate { duplicate_of: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{Decode, Encode};

    #[test]
    fn cycles_ledger_cid_and_principal_match() {
        assert_eq!(ICP_LEDGER_CID, ICP_LEDGER_PRINCIPAL.to_text());
    }

    // The types above stand in for `ic-ledger-types`, which `icp-project`
    // no longer depends on. These hold them to it, byte for byte.

    fn principals() -> [Principal; 3] {
        [
            Principal::anonymous(),
            Principal::management_canister(),
            Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
        ]
    }

    #[test]
    fn account_identifier_matches_ic_ledger_types() {
        let cmc = crate::cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL;
        for caller in principals() {
            let ours = AccountIdentifier::new(&cmc, &Subaccount::from(caller));
            let theirs = ic_ledger_types::AccountIdentifier::new(
                &cmc,
                &ic_ledger_types::Subaccount::from(caller),
            );
            assert_eq!(ours.as_ref(), theirs.as_ref());
        }
    }

    #[test]
    fn transfer_args_encode_as_ic_ledger_types_does() {
        let caller = principals()[2];
        let cmc = crate::cycles_minting_canister::CYCLES_MINTING_CANISTER_PRINCIPAL;
        let ours = TransferArgs {
            memo: Memo(7),
            amount: Tokens { e8s: 123 },
            fee: Tokens {
                e8s: ICP_LEDGER_BLOCK_FEE_E8S,
            },
            from_subaccount: Some(Subaccount::from(caller)),
            to: AccountIdentifier::new(&cmc, &Subaccount::from(caller)),
            created_at_time: Some(Timestamp {
                timestamp_nanos: 42,
            }),
        };
        let theirs = ic_ledger_types::TransferArgs {
            memo: ic_ledger_types::Memo(7),
            amount: ic_ledger_types::Tokens::from_e8s(123),
            fee: ic_ledger_types::Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
            from_subaccount: Some(ic_ledger_types::Subaccount::from(caller)),
            to: ic_ledger_types::AccountIdentifier::new(
                &cmc,
                &ic_ledger_types::Subaccount::from(caller),
            ),
            created_at_time: Some(ic_ledger_types::Timestamp {
                timestamp_nanos: 42,
            }),
        };
        assert_eq!(Encode!(&ours).unwrap(), Encode!(&theirs).unwrap());
    }

    #[test]
    fn transfer_result_decodes_what_ic_ledger_types_encodes() {
        use ic_ledger_types::{TransferError as E, TransferResult as R};
        let tokens = ic_ledger_types::Tokens::from_e8s;
        let cases: [(R, TransferResult); 6] = [
            (Ok(9), Ok(9)),
            (
                Err(E::BadFee {
                    expected_fee: tokens(1),
                }),
                Err(TransferError::BadFee {
                    expected_fee: Tokens { e8s: 1 },
                }),
            ),
            (
                Err(E::InsufficientFunds { balance: tokens(2) }),
                Err(TransferError::InsufficientFunds {
                    balance: Tokens { e8s: 2 },
                }),
            ),
            (
                Err(E::TxTooOld {
                    allowed_window_nanos: 3,
                }),
                Err(TransferError::TxTooOld {
                    allowed_window_nanos: 3,
                }),
            ),
            (
                Err(E::TxCreatedInFuture),
                Err(TransferError::TxCreatedInFuture),
            ),
            (
                Err(E::TxDuplicate { duplicate_of: 4 }),
                Err(TransferError::TxDuplicate { duplicate_of: 4 }),
            ),
        ];
        for (theirs, ours) in cases {
            let bytes = Encode!(&theirs).unwrap();
            assert_eq!(Decode!(&bytes, TransferResult).unwrap(), ours);
        }
    }
}
