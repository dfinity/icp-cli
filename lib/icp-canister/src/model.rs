use camino::Utf8Path;
use icp_adapter::{motoko::MotokoAdapter, rust::RustAdapter, script::ScriptAdapter};
use icp_fs::yaml::{LoadYamlFileError, load_yaml_file};
use serde::Deserialize;
use snafu::Snafu;

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
pub enum AdapterBuild {
    /// Represents a canister built using the Rust programming language.
    /// This variant holds the configuration specific to Rust-based builds.
    Rust(RustAdapter),

    /// Represents a canister built using the Motoko programming language.
    /// This variant holds the configuration specific to Motoko-based builds.
    Motoko(MotokoAdapter),

    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(ScriptAdapter),
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapter responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Build {
    pub adapter: AdapterBuild,
}

/// Represents one or more build steps.
/// This enum allows the `build` field in `canister.yaml` to be either
/// a single build configuration or a list of them.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum BuildSteps {
    Single(Build),
    Sequence(Vec<Build>),
}

impl BuildSteps {
    /// Consumes the enum and returns a `Vec<Build>`, ensuring
    /// the build logic can uniformly handle both single and multiple build steps.
    pub fn into_vec(self) -> Vec<Build> {
        match self {
            BuildSteps::Single(build) => vec![build],
            BuildSteps::Sequence(builds) => builds,
        }
    }
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
pub enum AdapterSync {
    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(ScriptAdapter),
}

/// Describes how the canister should be synced,
/// including the adapter responsible for the sync.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Sync {
    pub adapter: AdapterSync,
}

/// Represents one or more sync steps.
/// This enum allows the `sync` field in `canister.yaml` to be either
/// a single sync configuration or a list of them.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SyncSteps {
    Single(Sync),
    Sequence(Vec<Sync>),
}

impl SyncSteps {
    /// Consumes the enum and returns a `Vec<Sync>`, ensuring
    /// the sync logic can uniformly handle both single and multiple sync steps.
    pub fn into_vec(self) -> Vec<Sync> {
        match self {
            SyncSteps::Single(sync) => vec![sync],
            SyncSteps::Sequence(syncs) => syncs,
        }
    }
}

impl Default for SyncSteps {
    fn default() -> Self {
        Self::Sequence(vec![])
    }
}

/// Canister options, such as compute and memory allocation.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct CanisterOptions {
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

/// Configuration for canister creation
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Create {
    pub options: CanisterOptions,
}

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Debug, Deserialize, PartialEq)]
pub struct CanisterManifest {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    pub build: BuildSteps,

    /// The configuration specifying the various options when
    /// creating the canister.
    #[serde(default)]
    pub create: Create,

    /// The configuration specifying how to sync the canister
    #[serde(default)]
    pub sync: SyncSteps,
}

impl CanisterManifest {
    /// Loads a `CanisterManifest` from the specified YAML file path.
    pub fn load<P: AsRef<Utf8Path>>(path: P) -> Result<Self, LoadCanisterManifestError> {
        let path = path.as_ref();

        // Load the canister manifest from the YAML file.
        let cm: CanisterManifest = load_yaml_file(path)?;

        Ok(cm)
    }
}

#[derive(Debug, Snafu)]
pub enum LoadCanisterManifestError {
    #[snafu(transparent)]
    Parse { source: LoadYamlFileError },
}
