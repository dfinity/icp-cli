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
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Adapter {
    /// A canister written in Rust.
    Rust(RustAdapter),

    /// A canister written in Motoko.
    Motoko(MotokoAdapter),

    /// A canister built using a custom script or command.
    Script(ScriptAdapter),
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapter responsible for the build.
#[derive(Debug, Deserialize)]
pub struct Build {
    pub adapter: Adapter,
}

/// Represents the manifest describing a single canister,
/// including its name and how it should be built.
#[derive(Debug, Deserialize)]
pub struct CanisterManifest {
    /// Name of the canister described by this manifest.
    pub name: String,

    /// Build configuration for producing the canister's WebAssembly.
    pub build: Build,
}

impl CanisterManifest {
    pub fn from_file<P: AsRef<Utf8Path>>(path: P) -> Result<Self, LoadCanisterManifestError> {
        let path = path.as_ref();

        // Load
        let cm: CanisterManifest = load_yaml_file(path)?;

        Ok(cm)
    }
}

#[derive(Debug, Snafu)]
pub enum LoadCanisterManifestError {
    #[snafu(transparent)]
    Parse { source: LoadYamlFileError },
}
