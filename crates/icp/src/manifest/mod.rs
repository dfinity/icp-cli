use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use crate::prelude::*;

pub mod adapter;
pub mod canister;
pub mod dependency;
pub mod environment;
pub mod network;
pub mod project;
pub mod recipe;
pub(crate) mod serde_helpers;

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

#[derive(Debug, Snafu)]
pub enum ProjectRootLocateError {
    #[snafu(display("project manifest not found in {path}"))]
    NotFound { path: PathBuf },
}

/// Trait for locating the project root directory containing the project manifest file (`icp.yaml`).
pub trait ProjectRootLocate: Sync + Send {
    /// Locate the workspace root directory: the top-most project that
    /// transitively declares the project the command is standing in.
    fn locate(&self) -> Result<PathBuf, ProjectRootLocateError>;

    /// Locate the *member* directory the command is standing in: the nearest
    /// project manifest at or above cwd, without climbing to the workspace root.
    /// Equals [`locate`](Self::locate) at the root or in a standalone project.
    fn locate_member(&self) -> Result<PathBuf, ProjectRootLocateError>;
}
