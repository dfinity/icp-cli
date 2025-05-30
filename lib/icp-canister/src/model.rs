use serde::Deserialize;

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
