use std::{
    collections::{BTreeMap, HashMap},
    fmt,
};

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Visitor},
    ser::SerializeMap,
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

/// A single manifest path together with the map key it was declared under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedPath {
    /// The map key this path was declared under, or `None` for a plain-list
    /// entry. Non-unique: several paths share a key when the key maps to a list.
    pub key: Option<String>,
    /// The path itself, relative to the canister directory.
    pub path: String,
}

/// A set of manifest paths declared either as a plain list or as a map of
/// name → path(s). Used for a plugin step's `dirs` and `files`.
///
/// In `canister.yaml` this accepts three shapes:
/// ```yaml
/// # a plain list — entries carry no key
/// files:
///   - config.txt
///   - data.json
/// # a map whose keys each name a single path...
/// files:
///   main: config.txt
/// # ...or a list of paths, which all share that key
/// files:
///   seeds:
///     - a.json
///     - b.json
/// ```
///
/// Order is preserved: list entries in written order; map entries in written
/// key order, each key's paths in written order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamedPaths(Vec<NamedPath>);

/// A list of paths, or a map of name → path (or list of paths). The map form
/// tags each path with its key for the plugin; a key may map to several paths.
///
/// This type exists only to describe [`NamedPaths`] in the generated JSON schema
/// (see the `#[schemars(with = ...)]` on the adapter fields); [`NamedPaths`]
/// owns the actual (de)serialization.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum NamedPathsSchema {
    List(Vec<String>),
    Map(HashMap<String, PathOrListSchema>),
}

/// One map value in [`NamedPathsSchema`]: a single path, or a list of paths that
/// share the key.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum PathOrListSchema {
    One(String),
    Many(Vec<String>),
}

impl NamedPaths {
    /// Build from an ordered list of key-tagged paths.
    pub fn from_entries(entries: Vec<NamedPath>) -> Self {
        NamedPaths(entries)
    }

    /// The declared paths, in order, each tagged with its map key (if any).
    pub fn entries(&self) -> &[NamedPath] {
        &self.0
    }

    /// Consume into the ordered list of key-tagged paths.
    pub fn into_entries(self) -> Vec<NamedPath> {
        self.0
    }
}

impl FromIterator<NamedPath> for NamedPaths {
    fn from_iter<I: IntoIterator<Item = NamedPath>>(iter: I) -> Self {
        NamedPaths(iter.into_iter().collect())
    }
}

impl<'de> Deserialize<'de> for NamedPaths {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        /// A map value: a single path or a list of paths sharing the key.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PathOrList {
            One(String),
            Many(Vec<String>),
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            List(Vec<String>),
            Map(IndexMap<String, PathOrList>),
        }

        let entries = match Repr::deserialize(d)? {
            Repr::List(paths) => paths
                .into_iter()
                .map(|path| NamedPath { key: None, path })
                .collect(),
            Repr::Map(map) => map
                .into_iter()
                .flat_map(|(key, value)| {
                    let paths = match value {
                        PathOrList::One(path) => vec![path],
                        PathOrList::Many(paths) => paths,
                    };
                    paths.into_iter().map(move |path| NamedPath {
                        key: Some(key.clone()),
                        path,
                    })
                })
                .collect(),
        };
        Ok(NamedPaths(entries))
    }
}

impl Serialize for NamedPaths {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Deserialization yields either all-unkeyed (list form) or all-keyed
        // (map form) entries; serialize back to whichever it was.
        if self.0.iter().all(|e| e.key.is_none()) {
            let paths: Vec<&str> = self.0.iter().map(|e| e.path.as_str()).collect();
            paths.serialize(s)
        } else {
            // Group paths by key, preserving order. A key with one path
            // serializes as a scalar; multiple as a list.
            let mut groups: IndexMap<&str, Vec<&str>> = IndexMap::new();
            for e in &self.0 {
                groups
                    .entry(e.key.as_deref().unwrap_or_default())
                    .or_default()
                    .push(&e.path);
            }
            let mut map = s.serialize_map(Some(groups.len()))?;
            for (key, paths) in groups {
                match paths.as_slice() {
                    [one] => map.serialize_entry(key, one)?,
                    many => map.serialize_entry(key, many)?,
                }
            }
            map.end()
        }
    }
}

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

    /// Directories (relative to canister directory) the plugin may read from.
    /// Each entry must be a directory; it is preopened via WASI so the plugin
    /// can traverse it using standard filesystem APIs. Written as a plain list
    /// of paths, or as a map of name → path (or list of paths); the name is
    /// surfaced to the plugin as each entry's `key`.
    #[schemars(with = "Option<NamedPathsSchema>")]
    pub dirs: Option<NamedPaths>,

    /// Files (relative to canister directory) the host reads and passes to
    /// the plugin as part of `sync-exec-input.files`. Written as a plain list
    /// of paths, or as a map of name → path (or list of paths); the name is
    /// surfaced to the plugin as each entry's `key`.
    #[schemars(with = "Option<NamedPathsSchema>")]
    pub files: Option<NamedPaths>,

    /// Key-value fields passed to the plugin as part of `sync-exec-input.fields`.
    /// A plugin receives every value as a string; a number or boolean written
    /// unquoted arrives as its text form. The plugin decides how to interpret them.
    #[schemars(with = "Option<BTreeMap<String, FieldValue>>")]
    pub fields: Option<BTreeMap<String, String>>,

    /// Canisters this plugin may call in addition to the canister being synced.
    /// Each entry is a canister name resolved against the project's canister ID
    /// table for the environment being synced (e.g. `backend`, or a namespaced
    /// subproject canister such as `services/open-crm:backend`). The plugin
    /// picks a target per call via the `call-target` in its `canister-call`
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

    /// [`NamedPaths`] with no keys, as a plain-list manifest entry produces.
    fn unkeyed<const N: usize>(paths: [&str; N]) -> NamedPaths {
        NamedPaths::from_entries(
            paths
                .into_iter()
                .map(|path| NamedPath {
                    key: None,
                    path: path.to_string(),
                })
                .collect(),
        )
    }

    /// A single key-tagged [`NamedPath`].
    fn keyed(key: &str, path: &str) -> NamedPath {
        NamedPath {
            key: Some(key.to_string()),
            path: path.to_string(),
        }
    }
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
                dirs: Some(unkeyed(["assets/seed-data", "config"])),
                files: Some(unkeyed(["config.txt"])),
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
        assert_eq!(adapter.dirs, Some(unkeyed(["assets"])));
        assert_eq!(adapter.files, Some(unkeyed(["a.txt", "b.txt"])));
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
            adapter.dirs.map(NamedPaths::into_entries),
            Some(vec![
                keyed("seed", "assets/seed-data"),
                keyed("extra", "one"),
                keyed("extra", "two"),
            ]),
        );
        assert_eq!(
            adapter.files.map(NamedPaths::into_entries),
            Some(vec![keyed("main", "config.txt")]),
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
