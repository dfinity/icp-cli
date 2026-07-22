use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use crate::fs;
use crate::prelude::*;

pub mod adapter;
pub mod canister;
pub mod dependency;
pub mod environment;
pub mod network;
pub mod project;
pub mod recipe;
pub mod serde_helpers;

pub use {
    adapter::plugin,
    adapter::prebuilt,
    canister::{
        ArgsFormat, BuildStep, BuildSteps, CanisterManifest, Instructions, ManifestInitArgs,
        SyncStep, SyncSteps,
    },
    dependency::DependencyManifest,
    environment::EnvironmentManifest,
    network::{ManagedMode, Mode, NetworkManifest},
    project::ProjectManifest,
};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "canister.yaml";

#[derive(Debug, Snafu)]
pub enum LoadManifestError {
    #[snafu(transparent)]
    Read { source: fs::IoError },

    #[snafu(display("failed to parse manifest at '{path}'"))]
    Parse {
        source: serde_yaml::Error,
        path: PathBuf,
    },
}

/// Load and parse a YAML manifest of type `T` from disk.
pub fn load_manifest<T>(path: &Path) -> Result<T, LoadManifestError>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read(path)?;
    let m = serde_yaml::from_slice::<T>(&content).context(ParseSnafu {
        path: path.to_path_buf(),
    })?;
    Ok(m)
}

// A manifest item that can either be a path to another manifest file or the manifest itself.
//
// The valid path specifications are:
// - CanisterManifest: path or glob pattern to the directory containing "canister.yaml"
// - NetworkManifest: path to network manifest
// - EnvironmentManifest: path to environment manifest
#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
}

/// Items in path form serialize back to a bare path string, *not* to the contents of the
/// referenced file. Callers that need a self-contained YAML output (e.g. `icp project bundle`)
/// must convert any `Item::Path` to `Item::Manifest` themselves by loading the referenced
/// manifest first.
impl<T: Serialize> Serialize for Item<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Item::Path(p) => p.serialize(serializer),
            Item::Manifest(m) => m.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for Item<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor, value::MapAccessDeserializer};
        use std::fmt;

        struct ItemVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ItemVisitor<T> {
            type Value = Item<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string path or a manifest object")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Item::Path(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Item::Path(v))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                T::deserialize(MapAccessDeserializer::new(map)).map(Item::Manifest)
            }
        }

        deserializer.deserialize_any(ItemVisitor(PhantomData))
    }
}
