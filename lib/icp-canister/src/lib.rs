use serde::Deserialize;

/// Represents the manifest describing a single canister,
/// including its name and how it should be built.
#[derive(Debug, Deserialize)]
pub struct CanisterManifest {
    /// Name of the canister described by this manifest.
    pub name: String,

    /// Build configuration for producing the canister's WebAssembly.
    pub build: Build,
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapter responsible for the build.
#[derive(Debug, Deserialize)]
pub struct Build {
    pub adapter: Adapter,
}

/// Identifies the type of adapter used to build the canister,
/// e.g. "motoko", "rust", or "custom".
#[derive(Debug, Deserialize)]
pub struct Adapter {
    #[serde(rename = "type")]
    pub typ: AdapterType,
}

/// Known adapter types that can be used to build a canister.
/// These correspond to the values found in `build.adapter.type` in the YAML.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdapterType {
    /// A canister written in Rust.
    Rust,

    /// A canister written in Motoko.
    Motoko,

    /// A canister built using custom instructions,
    /// such as a shell script or other manual build process.
    Custom,

    /// An assets canister used to serve front-end applications
    /// or static assets on the Internet Computer.
    Assets,
}
