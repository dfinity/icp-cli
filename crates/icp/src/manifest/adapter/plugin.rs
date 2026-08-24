use std::{collections::BTreeMap, fmt};

use indexmap::IndexMap;
use itertools::Either;
use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Visitor},
};

use super::prebuilt::SourceField;

/// One `fields:` value on its way in from the manifest. A plugin always receives
/// a string, but writing `port: 8080` should not have to be quoted, so any YAML
/// scalar is accepted and stringified. Lists, mappings, and empty values are
/// rejected: there is no string to hand the plugin.
///
/// Note this cannot be left to serde's own `String` handling. `serde_yaml`
/// coerces scalars when deserializing straight from YAML text, but the manifest
/// is parsed into a `serde_yaml::Value` first (see `CanisterManifest`'s
/// `Deserialize`), and re-deserializing from a `Value` keeps a number a number.
struct FieldValue(String);

impl<'de> Deserialize<'de> for FieldValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ScalarVisitor;

        impl Visitor<'_> for ScalarVisitor {
            type Value = FieldValue;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string, number, or boolean")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<FieldValue, E> {
                Ok(FieldValue(v.to_owned()))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FieldValue, E> {
                Ok(FieldValue(v.to_string()))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FieldValue, E> {
                Ok(FieldValue(v.to_string()))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FieldValue, E> {
                Ok(FieldValue(v.to_string()))
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<FieldValue, E> {
                Ok(FieldValue(v.to_string()))
            }
        }

        d.deserialize_any(ScalarVisitor)
    }
}

impl JsonSchema for FieldValue {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FieldValue".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": ["string", "number", "boolean"],
        })
    }
}

fn deserialize_fields<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<BTreeMap<String, String>>, D::Error> {
    let fields = Option::<BTreeMap<String, FieldValue>>::deserialize(d)?;
    Ok(fields.map(|fields| fields.into_iter().map(|(k, v)| (k, v.0)).collect()))
}

/// The paths declared for a plugin step's `dirs` or `files`: either a plain list
/// of paths, or a map of name → path(s) whose keys are surfaced to the plugin.
///
/// ```yaml
/// # a plain list — entries carry no key
/// files:
///   - config.txt
///   - data.json
/// # a map whose keys each name a single path...
/// files:
///   main: config.txt
/// # ...or a list of paths, which then all share that key
/// files:
///   seeds:
///     - a.json
///     - b.json
/// ```
///
/// Order is preserved in both forms: list entries in written order; map entries
/// in written key order, each key's paths in written order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum NamedPaths {
    /// A plain list of paths, carrying no keys.
    List(Vec<String>),
    /// A map of name → path(s), tagging each path with the key it sits under.
    Map(IndexMap<String, PathOrList>),
}

/// One value of a [`NamedPaths::Map`]: a single path, or a list of paths that
/// all share the key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PathOrList {
    /// A single path under the key.
    One(String),
    /// Several paths, all sharing the key.
    Many(Vec<String>),
}

/// A declared path together with the map key it sits under, as yielded by
/// [`NamedPaths::entries`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedPath<'a> {
    /// The map key this path sits under, or `None` for a plain-list entry.
    /// Non-unique: the paths of a key that maps to a list all share it.
    pub key: Option<&'a str>,
    /// The path itself, relative to the canister directory.
    pub path: &'a str,
}

impl NamedPaths {
    /// The declared paths in written order, each tagged with its key (if any).
    pub fn entries(&self) -> impl Iterator<Item = NamedPath<'_>> {
        match self {
            Self::List(paths) => Either::Left(paths.iter().map(|path| NamedPath {
                key: None,
                path: path.as_str(),
            })),
            Self::Map(map) => Either::Right(map.iter().flat_map(|(key, value)| {
                value.paths().iter().map(move |path| NamedPath {
                    key: Some(key.as_str()),
                    path: path.as_str(),
                })
            })),
        }
    }

    /// Rewrite every path, leaving the keys and the written shape intact.
    pub fn map_paths(&self, mut f: impl FnMut(&str) -> String) -> Self {
        match self {
            Self::List(paths) => Self::List(paths.iter().map(|path| f(path)).collect()),
            Self::Map(map) => Self::Map(
                map.iter()
                    .map(|(key, value)| {
                        let value = match value {
                            PathOrList::One(path) => PathOrList::One(f(path)),
                            PathOrList::Many(paths) => {
                                PathOrList::Many(paths.iter().map(|path| f(path)).collect())
                            }
                        };
                        (key.clone(), value)
                    })
                    .collect(),
            ),
        }
    }
}

impl PathOrList {
    /// The paths sitting under this key.
    fn paths(&self) -> &[String] {
        match self {
            Self::One(path) => std::slice::from_ref(path),
            Self::Many(paths) => paths,
        }
    }
}

/// Configuration for a sync plugin step.
///
/// A sync plugin is a WebAssembly module invoked during `icp sync` for a
/// specific canister. It runs inside a WASI sandbox whose filesystem access
/// is limited to the directories listed in `dirs` (preopened read-only) plus
/// the contents of any files listed in `files` (read by the host and passed
/// inline to the plugin). Both are written relative to the canister directory
/// and may name anything inside the project, but nothing outside it.
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
///   fields:                             # key-value fields passed inline
///     api_url: https://example.com
///     retries: 3
/// ```
///
/// `dirs` and `files` may instead be written as a map, tagging each entry with a
/// `key` surfaced to the plugin; a key may map to a single path or a list:
/// ```yaml
/// - type: plugin
///   path: ./plugins/populate-data.wasm
///   dirs:
///     seed: assets/seed-data           # keyed single path
///     migrations:                      # keyed list — entries share the key
///       - migrations/2025
///       - migrations/2026
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

    /// Directories the plugin may read from, written relative to the canister
    /// directory and confined to the project (an entry may reach elsewhere in
    /// the project with `..`, but not out of it). Each entry must be a
    /// directory; it is made readable via WASI so the plugin can traverse it
    /// using standard filesystem APIs. Written as a plain list of paths, or as
    /// a map of name → path (or list of paths); the name is surfaced to the
    /// plugin as each entry's `key`. Entries may repeat a directory or name one
    /// inside another's — the plugin is told about each entry as written, and
    /// reads them all.
    pub dirs: Option<NamedPaths>,

    /// Files the host reads and passes to the plugin as part of
    /// `sync-exec-input.files`, written relative to the canister directory and
    /// confined to the project on the same terms as [`Self::dirs`]. Written as
    /// a plain list of paths, or as a map of name → path (or list of paths);
    /// the name is surfaced to the plugin as each entry's `key`.
    pub files: Option<NamedPaths>,

    /// Key-value fields passed to the plugin as part of `sync-exec-input.fields`.
    /// A plugin receives every value as a string; a number or boolean written
    /// unquoted arrives as its text form. The plugin decides how to interpret them.
    #[schemars(with = "Option<BTreeMap<String, FieldValue>>")]
    pub fields: Option<BTreeMap<String, String>>,

    /// Canisters this plugin may call, or read metadata from, in addition to
    /// the canister being synced. Each entry is a canister name resolved against
    /// the project's canister ID table for the environment being synced (e.g.
    /// `backend`, or a namespaced subproject canister such as
    /// `services/open-crm:backend`). The plugin picks a target per request via
    /// the `call-target` in its `canister-call` or `canister-metadata-section`
    /// request; a target not listed here is rejected by the host.
    pub canisters: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for Adapter {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct AdapterHelper {
            #[serde(flatten)]
            source: SourceField,
            sha256: Option<String>,
            dirs: Option<NamedPaths>,
            files: Option<NamedPaths>,
            #[serde(default, deserialize_with = "deserialize_fields")]
            fields: Option<BTreeMap<String, String>>,
            canisters: Option<Vec<String>>,
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
            fields: h.fields,
            canisters: h.canisters,
        })
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    use crate::manifest::adapter::prebuilt::{LocalSource, RemoteSource};

    /// The plain-list form of `dirs:`/`files:`.
    fn list<const N: usize>(paths: [&str; N]) -> NamedPaths {
        NamedPaths::List(paths.into_iter().map(str::to_string).collect())
    }

    /// A key-tagged entry, as [`NamedPaths::entries`] yields for the map form.
    fn keyed<'a>(key: &'a str, path: &'a str) -> NamedPath<'a> {
        NamedPath {
            key: Some(key),
            path,
        }
    }

    /// The flattened entries of an optional `dirs:`/`files:` setting.
    fn entries(paths: &Option<NamedPaths>) -> Option<Vec<NamedPath<'_>>> {
        paths.as_ref().map(|paths| paths.entries().collect())
    }

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
                fields: None,
                canisters: None,
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
                dirs: Some(list(["assets/seed-data", "config"])),
                files: Some(list(["config.txt"])),
                fields: None,
                canisters: None,
            },
        );
    }

    /// Parse an adapter the way the manifest loader does: YAML text into a
    /// `serde_yaml::Value`, then that value into the typed adapter. Going
    /// through the value matters for `fields` — deserializing straight from
    /// text lets `serde_yaml` coerce scalars to strings on its own, which
    /// would hide whether `FieldValue` accepts them.
    fn adapter_via_value(yaml: &str) -> Result<Adapter, serde_yaml::Error> {
        let value: serde_yaml::Value = serde_yaml::from_str(yaml).expect("invalid yaml");
        serde_yaml::from_value(value)
    }

    /// The list form leaves every entry keyless.
    #[test]
    fn dirs_and_files_as_plain_lists_have_no_keys() {
        let adapter = serde_yaml::from_str::<Adapter>(
            r#"
            path: plugins/my-sync.wasm
            dirs:
              - assets
            files:
              - a.txt
              - b.txt
            "#,
        )
        .expect("failed to deserialize Adapter with list dirs/files");
        assert_eq!(adapter.dirs, Some(list(["assets"])));
        assert_eq!(adapter.files, Some(list(["a.txt", "b.txt"])));
    }

    /// The map form tags each entry with its key. A key mapping to a list yields
    /// several entries sharing that (non-unique) key, in written order.
    #[test]
    fn dirs_and_files_as_maps_carry_keys() {
        let adapter = serde_yaml::from_str::<Adapter>(
            r#"
            path: plugins/my-sync.wasm
            dirs:
              seed: assets/seed-data
              extra:
                - one
                - two
            files:
              main: config.txt
            "#,
        )
        .expect("failed to deserialize Adapter with map dirs/files");
        assert_eq!(
            entries(&adapter.dirs),
            Some(vec![
                keyed("seed", "assets/seed-data"),
                keyed("extra", "one"),
                keyed("extra", "two"),
            ]),
        );
        assert_eq!(
            entries(&adapter.files),
            Some(vec![keyed("main", "config.txt")]),
        );
    }

    /// Rewriting paths (as bundling does) leaves keys and the written shape alone.
    #[test]
    fn map_paths_preserves_keys_and_shape() {
        let paths: NamedPaths = serde_yaml::from_str("single: one.txt\nmany:\n- x.txt\n- y.txt\n")
            .expect("failed to parse NamedPaths");
        let mapped = paths.map_paths(|path| format!("bundled/{path}"));
        assert_eq!(
            serde_yaml::to_string(&mapped).expect("failed to serialize"),
            "single: bundled/one.txt\nmany:\n- bundled/x.txt\n- bundled/y.txt\n",
        );
    }

    /// The list and map forms round-trip through serialization back to their
    /// natural YAML shape.
    #[test]
    fn named_paths_round_trip() {
        for yaml in [
            "- a.txt\n- b.txt\n",
            "single: one.txt\nmany:\n- x.txt\n- y.txt\n",
        ] {
            let parsed: NamedPaths =
                serde_yaml::from_str(yaml).expect("failed to parse NamedPaths");
            let reserialized = serde_yaml::to_string(&parsed).expect("failed to serialize");
            assert_eq!(reserialized, yaml, "round-trip changed the YAML shape");
        }
    }

    #[test]
    fn fields_parse_as_a_string_map() {
        let adapter = adapter_via_value(
            r#"
            path: plugins/my-sync.wasm
            fields:
              api_url: https://example.com
              token: abc123
            "#,
        )
        .expect("failed to deserialize Adapter with fields");
        assert_eq!(
            adapter.fields,
            Some(BTreeMap::from([
                ("api_url".to_string(), "https://example.com".to_string()),
                ("token".to_string(), "abc123".to_string()),
            ])),
        );
    }

    #[test]
    fn scalar_field_values_are_stringified() {
        let adapter = adapter_via_value(
            r#"
            path: plugins/my-sync.wasm
            fields:
              port: 8080
              enabled: true
              ratio: 1.5
            "#,
        )
        .expect("failed to deserialize Adapter with scalar fields");
        assert_eq!(
            adapter.fields,
            Some(BTreeMap::from([
                ("port".to_string(), "8080".to_string()),
                ("enabled".to_string(), "true".to_string()),
                ("ratio".to_string(), "1.5".to_string()),
            ])),
        );
    }

    #[test]
    fn non_scalar_field_values_are_rejected() {
        for yaml in [
            // A plugin can only receive a string, so there is nothing sensible
            // to hand it for a nested mapping...
            indoc! {r#"
                path: plugins/my-sync.wasm
                fields:
                  nested:
                    a: b
            "#},
            // ...or for a key written with no value at all.
            indoc! {r#"
                path: plugins/my-sync.wasm
                fields:
                  blank:
            "#},
        ] {
            let err =
                adapter_via_value(yaml).expect_err("non-scalar field value should be rejected");
            assert!(
                err.to_string()
                    .contains("expected a string, number, or boolean"),
                "unexpected error: {err}"
            );
        }
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
    fn canisters_parse_as_names() {
        let adapter = serde_yaml::from_str::<Adapter>(
            r#"
            path: plugins/my-sync.wasm
            canisters:
              - backend
              - services/open-crm:backend
            "#,
        )
        .expect("failed to deserialize Adapter with canisters");
        assert_eq!(
            adapter.canisters,
            Some(vec![
                "backend".to_string(),
                "services/open-crm:backend".to_string(),
            ]),
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
                fields: None,
                canisters: None,
            },
        );
    }
}
