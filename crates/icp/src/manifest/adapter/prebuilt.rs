use std::fmt;

use crate::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct LocalSource {
    /// Local path on-disk to read a WASM file from
    #[schemars(with = "String")]
    pub path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct RemoteSource {
    /// Url to fetch the remote WASM file from
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(untagged, rename_all = "lowercase")]
pub enum SourceField {
    /// Local path on-disk to read a WASM file from
    Local(LocalSource),

    /// Remote url to fetch a WASM file from
    Remote(RemoteSource),
}

/// Configuration for a Pre-built canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
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

        write!(f, "{src}, sha: {sha}")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn path() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                path: canister.wasm
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Local(LocalSource {
                    path: "canister.wasm".into()
                }),
                sha256: None,
            },
        );
    }

    #[test]
    fn path_with_sha256() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                path: canister.wasm
                sha256: sha256
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Local(LocalSource {
                    path: "canister.wasm".into()
                }),
                sha256: Some("sha256".to_string()),
            },
        );
    }

    #[test]
    fn url() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                url: http://example.com/canister.wasm
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Remote(RemoteSource {
                    url: "http://example.com/canister.wasm".to_string(),
                }),
                sha256: None,
            },
        );
    }

    #[test]
    fn url_with_sha256() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                url: http://example.com/canister.wasm
                sha256: sha256
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Remote(RemoteSource {
                    url: "http://example.com/canister.wasm".to_string(),
                }),
                sha256: Some("sha256".to_string()),
            },
        );
    }
}
