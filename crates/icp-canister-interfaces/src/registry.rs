use candid::{CandidType, Deserialize, Principal};

pub const REGISTRY_CID: &str = "rwlgt-iiaaa-aaaaa-aaaaa-cai";
pub const REGISTRY_PRINCIPAL: Principal = Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 1, 1]);

#[derive(CandidType, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GetSubnetForCanisterRequest {
    pub principal: Option<Principal>,
}

#[derive(CandidType, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GetSubnetForCanisterSuccess {
    pub subnet_id: Option<Principal>,
}

pub type GetSubnetForCanisterResult = Result<GetSubnetForCanisterSuccess, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_cid_and_principal_match() {
        assert_eq!(REGISTRY_CID, REGISTRY_PRINCIPAL.to_text());
    }
}
