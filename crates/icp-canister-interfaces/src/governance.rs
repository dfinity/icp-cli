use candid::Principal;

pub const GOVERNANCE_CID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";
pub const GOVERNANCE_PRINCIPAL: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 1, 1]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_ledger_cid_and_principal_match() {
        assert_eq!(GOVERNANCE_CID, GOVERNANCE_PRINCIPAL.to_text());
    }
}
