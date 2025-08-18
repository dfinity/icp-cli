use std::collections::HashMap;

use serde::Deserialize;

use crate::{BuildSteps, CanisterSettings, SyncSteps};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RecipeType {
    Assets,
    Motoko,
    Rust,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Recipe {
    #[serde(rename = "type")]
    pub recipe_type: RecipeType,

    #[serde(rename = "configuration")]
    pub instructions: HashMap<String, serde_yaml::Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CanisterInstructions {
    Recipe {
        recipe: Recipe,
    },

    BuildSync {
        /// The build configuration specifying how to compile the canister's source
        /// code into a WebAssembly module, including the adapter to use.
        build: BuildSteps,

        /// The configuration specifying how to sync the canister
        #[serde(default)]
        sync: SyncSteps,
    },
}

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CanisterManifest {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The configuration specifying the various settings when
    /// creating the canister.
    #[serde(default)]
    pub settings: CanisterSettings,

    #[serde(flatten)]
    pub instructions: CanisterInstructions,
}
