use std::path::{Path, PathBuf};

use serde::Deserialize;
use snafu::{Snafu, ensure};

use icp_fs::fs::{ReadFileError, read};

/// Configuration for a Rust-based canister build adapter.
#[derive(Debug, Deserialize)]
pub struct RustAdapter {
    /// The name of the Cargo package to build.
    pub package: String,
}

/// Configuration for a Motoko-based canister build adapter.
#[derive(Debug, Deserialize)]
pub struct MotokoAdapter {
    /// Optional path to the main Motoko source file.
    /// If omitted, a default like `main.mo` may be assumed.
    #[serde(default)]
    pub main: Option<String>,
}

/// Configuration for a custom canister build adapter.
#[derive(Debug, Deserialize)]
pub struct CustomAdapter {
    /// Path to a script or executable used to build the canister.
    pub script: String,
}

/// Configuration for an asset canister used to serve static content.
#[derive(Debug, Deserialize)]
pub struct AssetsAdapter {
    /// Directory containing the static assets to include in the canister.
    pub source: String,
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
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Adapter {
    /// A canister written in Rust.
    Rust(RustAdapter),

    /// A canister written in Motoko.
    Motoko(MotokoAdapter),

    /// A canister built using a custom script or command.
    Custom(CustomAdapter),

    /// A static asset canister.
    Assets(AssetsAdapter),
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
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, CanisterManifestError> {
        let path = path.as_ref();

        // Check existence
        ensure!(path.exists(), NotFoundSnafu { path });

        // Read
        let bytes = read(path)?;

        // Parse
        let cm: CanisterManifest = serde_yaml::from_slice(bytes.as_ref())?;

        Ok(cm)
    }
}

#[derive(Debug, Snafu)]
pub enum CanisterManifestError {
    #[snafu(display("canister manifest not found: {path:?}"))]
    NotFound { path: PathBuf },

    #[snafu(transparent)]
    Parse { source: serde_yaml::Error },

    #[snafu(transparent)]
    ReadFile { source: ReadFileError },
}
