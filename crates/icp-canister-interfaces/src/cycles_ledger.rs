use candid::Principal;

/// 100m cycles
pub const CYCLES_LEDGER_BLOCK_FEE: u128 = 100_000_000;

pub const CYCLES_LEDGER_CID: &str = "um5iw-rqaaa-aaaaq-qaaba-cai";
pub const CYCLES_LEDGER_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 2, 1, 1]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_ledger_cid_and_principal_match() {
        assert_eq!(CYCLES_LEDGER_CID, CYCLES_LEDGER_PRINCIPAL.to_text());
    }
}
