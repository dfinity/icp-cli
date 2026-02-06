use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

/// Arguments for the proxy canister's `proxy` method.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ProxyArgs {
    /// The target canister to forward the call to.
    pub canister_id: Principal,
    /// The method name to invoke on the target canister.
    pub method: String,
    /// The serialized Candid arguments for the method.
    pub args: Vec<u8>,
    /// The number of cycles to forward with the call.
    pub cycles: Nat,
}

/// Result from the proxy canister's `proxy` method.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum ProxyResult {
    /// The proxied call succeeded.
    Ok(ProxyOk),
    /// The proxied call failed.
    Err(ProxyError),
}

/// Success result containing the response from the target canister.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ProxyOk {
    /// The serialized Candid response from the target canister.
    pub result: Vec<u8>,
}

/// Error variants from the proxy canister.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum ProxyError {
    /// The proxy canister does not have enough cycles to process the request.
    InsufficientCycles {
        /// The number of cycles available.
        available: Nat,
        /// The number of cycles required.
        required: Nat,
    },
    /// The call to the target canister failed.
    CallFailed {
        /// A description of the failure reason.
        reason: String,
    },
    /// The caller is not authorized to use this proxy canister.
    UnauthorizedUser,
}

impl ProxyError {
    /// Format the error for display.
    pub fn format_error(&self) -> String {
        match self {
            ProxyError::InsufficientCycles {
                available,
                required,
            } => {
                format!(
                    "Proxy canister has insufficient cycles. Available: {available}, required: {required}"
                )
            }
            ProxyError::CallFailed { reason } => {
                format!("Proxy call failed: {reason}")
            }
            ProxyError::UnauthorizedUser => {
                "Unauthorized: you are not in the proxy canister's controllers list".to_string()
            }
        }
    }
}
