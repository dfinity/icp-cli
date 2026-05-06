use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use super::prebuilt::SourceField;

/// Configuration for a sync plugin step.
///
/// A sync plugin is a WebAssembly module invoked during `icp sync` for a
/// specific canister. It runs inside a WASI sandbox whose filesystem access
/// is limited to the directories listed in `dirs` (preopened read-only) plus
/// the contents of any files listed in `files` (read by the host and passed
/// inline to the plugin).
///
/// Example (local path):
/// ```yaml
/// - type: plugin
///   path: ./plugins/populate-data.wasm
///   sha256: e3b0c44298fc1c149afb...   # optional for path
///   dirs:                               # directories preopened read-only
///     - assets/seed-data
///   files:                              # files read by the host and passed inline
///     - config.txt
/// ```
///
/// Example (remote URL — `sha256` is required):
/// ```yaml
/// - type: plugin
///   url: https://example.com/plugins/populate-data.wasm
///   sha256: e3b0c44298fc1c149afb...   # required for url
/// ```
#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize)]
pub struct Adapter {
    #[serde(flatten)]
    pub source: SourceField,

    /// Optional sha256 checksum of the wasm file.
    /// Optional for `path`; required for `url`.
    pub sha256: Option<String>,

    /// Directories (relative to canister directory) the plugin may read from.
    /// Each entry must be a directory; it is preopened via WASI so the plugin
    /// can traverse it using standard filesystem APIs.
    pub dirs: Option<Vec<String>>,

    /// Files (relative to canister directory) the host reads and passes to
    /// the plugin as part of `sync-exec-input.files`.
    pub files: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for Adapter {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct AdapterHelper {
            #[serde(flatten)]
            source: SourceField,
            sha256: Option<String>,
            dirs: Option<Vec<String>>,
            files: Option<Vec<String>>,
        }

        let h = AdapterHelper::deserialize(d)?;
        if matches!(h.source, SourceField::Remote(_)) && h.sha256.is_none() {
            return Err(serde::de::Error::custom(
                "plugin with `url` requires `sha256` for integrity verification",
            ));
        }
        Ok(Self {
            source: h.source,
            sha256: h.sha256,
            dirs: h.dirs,
            files: h.files,
        })
    }
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
                files: None,
            },
        );
    }

    #[test]
    fn local_path_with_sha256_dirs_and_files() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                path: plugins/my-sync.wasm
                sha256: abc123
                dirs:
                  - assets/seed-data
                  - config
                files:
                  - config.txt
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                source: SourceField::Local(LocalSource {
                    path: "plugins/my-sync.wasm".into(),
                }),
                sha256: Some("abc123".to_string()),
                dirs: Some(vec!["assets/seed-data".to_string(), "config".to_string()]),
                files: Some(vec!["config.txt".to_string()]),
            },
        );
    }

    #[test]
    fn remote_url_without_sha256_is_rejected() {
        let err = serde_yaml::from_str::<Adapter>(
            r#"
            url: https://example.com/plugins/migrate-v2.wasm
            "#,
        )
        .expect_err("expected error for remote url without sha256");
        assert!(
            err.to_string()
                .contains("plugin with `url` requires `sha256`"),
            "unexpected error: {err}"
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
                files: None,
            },
        );
    }
}
