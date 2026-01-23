use candid::Principal;

/// 0.0001 ICP, a.k.a. 10k e8s
pub const ICP_LEDGER_BLOCK_FEE_E8S: u64 = 10_000;
pub const ICP_LEDGER_SYMBOL: &str = "ICP";
pub const ICP_LEDGER_CID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const ICP_LEDGER_PRINCIPAL: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 2, 1, 1]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_ledger_cid_and_principal_match() {
        assert_eq!(ICP_LEDGER_CID, ICP_LEDGER_PRINCIPAL.to_text());
    }
}
