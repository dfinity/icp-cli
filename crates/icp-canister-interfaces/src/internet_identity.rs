use candid::Principal;

pub const INTERNET_IDENTITY_FRONTEND_CID: &str = "uqzsh-gqaaa-aaaaq-qaada-cai";
pub const INTERNET_IDENTITY_FRONTEND_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 6, 1, 1]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internet_identity_frontend_cid_and_principal_match() {
        assert_eq!(
            INTERNET_IDENTITY_FRONTEND_CID,
            INTERNET_IDENTITY_FRONTEND_PRINCIPAL.to_text()
        );
    }
}
