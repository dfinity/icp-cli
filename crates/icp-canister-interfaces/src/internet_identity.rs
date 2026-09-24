use candid::{CandidType, Principal};
use serde::Deserialize;

pub const INTERNET_IDENTITY_FRONTEND_CID: &str = "uqzsh-gqaaa-aaaaq-qaada-cai";
pub const INTERNET_IDENTITY_FRONTEND_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 6, 1, 1]);
pub const INTERNET_IDENTITY_CID: &str = "rdmx6-jaaaa-aaaaa-aaadq-cai";
pub const INTERNET_IDENTITY_PRINCIPAL: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 7, 1, 1]);

/// Result of `lookup_device_key`.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct DeviceKeyWithAnchor {
    pub pubkey: Vec<u8>,
    pub anchor_number: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IdRegNextStepResult {
    pub next_step: RegistrationFlowNextStep,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum RegistrationFlowNextStep {
    CheckCaptcha { captcha_png_base64: String },
    Finish,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum IdRegStartError {
    InvalidCaller,
    RateLimitExceeded,
    AlreadyInProgress,
}

/// Argument of `identity_registration_finish`.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IdRegFinishArg {
    pub authn_method: AuthnMethodData,
    pub name: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct AuthnMethodData {
    pub authn_method: AuthnMethod,
    pub security_settings: AuthnMethodSecuritySettings,
    pub metadata: Vec<(String, MetadataEntryV2)>,
    pub last_authentication: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum AuthnMethod {
    WebAuthn(WebAuthn),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct WebAuthn {
    pub pubkey: Vec<u8>,
    pub credential_id: Vec<u8>,
    pub aaguid: Option<Vec<u8>>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct AuthnMethodSecuritySettings {
    pub protection: AuthnMethodProtection,
    pub purpose: AuthnMethodPurpose,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum AuthnMethodProtection {
    Protected,
    Unprotected,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum AuthnMethodPurpose {
    Recovery,
    Authentication,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum MetadataEntryV2 {
    String(String),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IdRegFinishResult {
    pub identity_number: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum IdRegFinishError {
    UnexpectedCall { next_step: RegistrationFlowNextStep },
    NoRegistrationFlow,
    InvalidAuthnMethod(String),
    StorageError(String),
    SsoNormalLoginRequired,
}

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
