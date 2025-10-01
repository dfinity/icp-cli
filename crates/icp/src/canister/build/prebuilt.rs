use std::fmt;

use crate::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct LocalSource {
    /// Local path on-disk to read a WASM file from
    #[schemars(with = "String")]
    pub path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct RemoteSource {
    /// Url to fetch the remote WASM file from
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged, rename_all = "lowercase")]
pub enum SourceField {
    /// Local path on-disk to read a WASM file from
    Local(LocalSource),

    /// Remote url to fetch a WASM file from
    Remote(RemoteSource),
}

/// Configuration for a Pre-built canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Adapter {
    #[serde(flatten)]
    pub source: SourceField,

    /// Optional sha256 checksum of the WASM
    pub sha256: Option<String>,
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let src = match &self.source {
            SourceField::Local(v) => format!("path: {}", v.path),
            SourceField::Remote(v) => format!("url: {}", v.url),
        };

        let sha = match &self.sha256 {
            Some(v) => v,
            None => "n/a",
        };

        write!(f, "({src}, sha: {sha})")
    }
}
