use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::prebuilt::SourceField;

/// Configuration for a sync plugin step.
///
/// A sync plugin is a WebAssembly module invoked during `icp sync` for a
/// specific canister.  It runs inside the Extism sandbox with restricted
/// permissions — it can only call canister methods on the canister being
/// synced and read files from the declared `dirs` allowlist.
///
/// Example:
/// ```yaml
/// - type: plugin
///   path: ./plugins/populate-data.wasm
///   sha256: e3b0c44298fc1c149afb...   # optional but recommended
///   dirs:                               # optional read-access directories
///     - assets/seed-data/
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Adapter {
    #[serde(flatten)]
    pub source: SourceField,

    /// Optional sha256 checksum of the wasm file.
    /// Required when `url` is used; optional (but recommended) for `path`.
    pub sha256: Option<String>,

    /// Directories (relative to canister directory) the plugin may read from.
    pub dirs: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::adapter::prebuilt::{LocalSource, RemoteSource};

    #[test]
    fn local_path() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                path: plugins/my-sync.wasm
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Local(LocalSource {
                    path: "plugins/my-sync.wasm".into(),
                }),
                sha256: None,
                dirs: None,
            },
        );
    }

    #[test]
    fn local_path_with_sha256_and_dirs() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                path: plugins/my-sync.wasm
                sha256: abc123
                dirs:
                  - assets/seed-data/
                  - config/
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Local(LocalSource {
                    path: "plugins/my-sync.wasm".into(),
                }),
                sha256: Some("abc123".to_string()),
                dirs: Some(vec!["assets/seed-data/".to_string(), "config/".to_string(),]),
            },
        );
    }

    #[test]
    fn remote_url_with_sha256() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                url: https://example.com/plugins/migrate-v2.wasm
                sha256: a665a45920422f9d417e
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Remote(RemoteSource {
                    url: "https://example.com/plugins/migrate-v2.wasm".to_string(),
                }),
                sha256: Some("a665a45920422f9d417e".to_string()),
                dirs: None,
            },
        );
    }
}
