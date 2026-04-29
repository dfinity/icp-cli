use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::manifest::canister::ManifestInitArgs;

/// Configuration for a canister call sync step.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Adapter {
    /// Name of the canister in the current project to call.
    pub canister: String,

    /// Name of the canister method to invoke.
    pub method: String,

    /// Arguments to pass to the method call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<ManifestInitArgs>,
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(canister: {}, method: {})", self.canister, self.method)
    }
}
