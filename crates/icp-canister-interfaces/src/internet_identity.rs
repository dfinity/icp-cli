use candid::Principal;

pub const INTERNET_IDENTITY_FRONTEND_CID: &str = "uqzsh-gqaaa-aaaaq-qaada-cai";
pub const INTERNET_IDENTITY_FRONTEND_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 6, 1, 1]);
pub const INTERNET_IDENTITY_CID: &str = "rdmx6-jaaaa-aaaaa-aaadq-cai";
pub const INTERNET_IDENTITY_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 7, 1, 1]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internet_identity_cid_and_principal_match() {
        assert_eq!(INTERNET_IDENTITY_CID, INTERNET_IDENTITY_PRINCIPAL.to_text());
    }

    #[test]
    fn internet_identity_frontend_cid_is_valid() {
        assert_eq!(
            INTERNET_IDENTITY_FRONTEND_CID,
            INTERNET_IDENTITY_FRONTEND_PRINCIPAL.to_text()
        );
    }
}
