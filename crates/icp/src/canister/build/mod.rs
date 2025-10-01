use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::manifest::adapter::{prebuilt, script};

/// Identifies the type of adapter used to build the canister,
/// along with its configuration.
///
/// The adapter type is specified via the `type` field in the YAML file.
/// For example:
///
/// ```yaml
/// type: rust
/// package: my_canister
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Step {
    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(script::Adapter),

    /// Represents a pre-built canister.
    /// This variant allows for retrieving a canister WASM from various sources.
    #[serde(rename = "pre-built")]
    Prebuilt(prebuilt::Adapter),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Step::Script(v) => format!("script {v}"),
                Step::Prebuilt(v) => format!("pre-built {v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Steps {
    pub steps: Vec<Step>,
}
