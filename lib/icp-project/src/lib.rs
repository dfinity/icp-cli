use icp_canister::{BuildSteps, CanisterSettings, SyncSteps};
use serde::Deserialize;

pub mod directory;
pub mod model;
pub mod project;
pub mod structure;

pub const ENVIRONMENT_LOCAL: &str = "local";
pub const ENVIRONMENT_IC: &str = "ic";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CanisterManifest {
    /// Canister name
    pub name: String,

    /// Canister settings
    #[serde(default)]
    pub settings: CanisterSettings,

    /// Canister build instructions
    build: BuildSteps,

    /// Canister sync instructions
    #[serde(default)]
    sync: SyncSteps,
}
