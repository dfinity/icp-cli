pub use directories::{Directories, DirectoriesError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::canister::{Settings, build, sync};

pub mod canister;
mod directories;
pub mod fs;
mod manifest;
pub mod prelude;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Canister {
    pub name: String,

    #[serde(default)]
    pub settings: Settings,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    build: build::Steps,

    /// The configuration specifying how to sync the canister
    #[serde(default)]
    sync: sync::Steps,
}

pub struct Network {}

pub struct Environment {}

pub struct Project {
    pub canisters: Vec<Canister>,
    pub networks: Vec<Network>,
    pub environments: Vec<Environment>,
}
