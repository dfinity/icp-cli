use icp_adapter::{
    assets::AssetsAdapter, motoko::MotokoAdapter, pre_built::PrebuiltAdapter, rust::RustAdapter,
    script::ScriptAdapter,
};
use serde::Deserialize;

pub use manifest::{CanisterInstructions, CanisterManifest, Recipe};

pub mod handlebars;
pub mod manifest;
pub mod recipe;

/// Canister settings, such as compute and memory allocation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct CanisterSettings {
    /// Compute allocation (0 to 100). Represents guaranteed compute capacity.
    pub compute_allocation: Option<u64>,

    /// Memory allocation in bytes. If unset, memory is allocated dynamically.
    pub memory_allocation: Option<u64>,

    /// Freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    pub freezing_threshold: Option<u64>,

    /// Reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    pub reserved_cycles_limit: Option<u64>,

    /// Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    pub wasm_memory_limit: Option<u64>,

    /// Wasm memory threshold in bytes. Triggers a callback when exceeded.
    pub wasm_memory_threshold: Option<u64>,
}

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
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BuildStep {
    /// Represents a canister built using the Rust programming language.
    /// This variant holds the configuration specific to Rust-based builds.
    Rust(RustAdapter),

    /// Represents a canister built using the Motoko programming language.
    /// This variant holds the configuration specific to Motoko-based builds.
    Motoko(MotokoAdapter),

    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(ScriptAdapter),

    /// Represents a pre-built canister.
    /// This variant allows for retrieving a canister WASM from various sources.
    #[serde(rename = "pre-built")]
    Prebuilt(PrebuiltAdapter),
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct BuildSteps {
    pub steps: Vec<BuildStep>,
}

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
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SyncStep {
    /// Represents a canister synced using a custom script or command.
    /// This variant allows for flexible sync processes defined by the user.
    Script(ScriptAdapter),

    /// Represents syncing of an assets canister
    Assets(AssetsAdapter),
}

/// Describes how the canister should be synced,
/// including the adapters and steps responsible for the sync.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SyncSteps {
    pub steps: Vec<SyncStep>,
}
