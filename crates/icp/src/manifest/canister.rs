use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    canister::{Settings, build, sync},
    manifest::recipe::Recipe,
};

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Instructions {
    Recipe {
        recipe: Recipe,
    },

    BuildSync {
        /// The build configuration specifying how to compile the canister's source
        /// code into a WebAssembly module, including the adapter to use.
        build: build::Steps,

        /// The configuration specifying how to sync the canister
        #[serde(default)]
        sync: sync::Steps,
    },
}

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Canister {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The configuration specifying the various settings when
    /// creating the canister.
    #[serde(default)]
    pub settings: Settings,

    #[serde(flatten)]
    pub instructions: Instructions,
}
