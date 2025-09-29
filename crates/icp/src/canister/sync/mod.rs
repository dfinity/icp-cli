use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

pub mod assets;
pub mod script;

/// Identifies the type of adapter used to sync the canister,
/// along with its configuration.
///
/// The adapter type is specified via the `type` field in the YAML file.
/// For example:
///
/// ```yaml
/// type: script
/// command: echo "synchronizing canister"
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Step {
    /// Represents a canister synced using a custom script or command.
    /// This variant allows for flexible sync processes defined by the user.
    Script(script::Adapter),

    /// Represents syncing of an assets canister
    Assets(assets::Adapter),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Step::Script(v) => format!("script {v}"),
                Step::Assets(v) => format!("assets {v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub struct Steps {
    pub steps: Vec<Step>,
}
