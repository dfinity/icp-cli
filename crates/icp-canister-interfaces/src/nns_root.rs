use candid::Principal;

pub const NNS_ROOT_CID: &str = "r7inp-6aaaa-aaaaa-aaabq-cai";
pub const NNS_ROOT_PRINCIPAL: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 3, 1, 1]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nns_root_cid_and_principal_match() {
        assert_eq!(NNS_ROOT_CID, NNS_ROOT_PRINCIPAL.to_text());
    }
}
