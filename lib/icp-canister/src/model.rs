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
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Adapter {
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
#[derive(Debug, Deserialize, PartialEq)]
pub struct Build {
    pub adapter: Adapter,
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
    pub build: Build,
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
