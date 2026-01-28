use std::collections::HashMap;

use candid::Nat;
use icp_canister_interfaces::cycles_ledger::CanisterSettingsArg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod build;
pub mod recipe;
pub mod sync;

mod script;

/// Controls who can read canister logs.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogVisibility {
    /// Only controllers can view logs.
    #[default]
    Controllers,
    /// Anyone can view logs.
    Public,
}

impl From<LogVisibility> for ic_management_canister_types::LogVisibility {
    fn from(value: LogVisibility) -> Self {
        match value {
            LogVisibility::Controllers => ic_management_canister_types::LogVisibility::Controllers,
            LogVisibility::Public => ic_management_canister_types::LogVisibility::Public,
        }
    }
}

impl From<LogVisibility> for icp_canister_interfaces::cycles_ledger::LogVisibility {
    fn from(value: LogVisibility) -> Self {
        use icp_canister_interfaces::cycles_ledger::LogVisibility as CyclesLedgerLogVisibility;
        match value {
            LogVisibility::Controllers => CyclesLedgerLogVisibility::Controllers,
            LogVisibility::Public => CyclesLedgerLogVisibility::Public,
        }
    }
}

/// Canister settings, such as compute and memory allocation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Settings {
    /// Controls who can read canister logs.
    pub log_visibility: Option<LogVisibility>,

    /// Compute allocation (0 to 100). Represents guaranteed compute capacity.
    pub compute_allocation: Option<u64>,

    /// Memory allocation in bytes. If unset, memory is allocated dynamically.
    pub memory_allocation: Option<u64>,

    /// Freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    pub freezing_threshold: Option<u64>,

    /// Reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    pub reserved_cycles_limit: Option<u64>,

    /// Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    pub wasm_memory_limit: Option<u64>,

    /// Wasm memory threshold in bytes. Triggers a callback when exceeded.
    pub wasm_memory_threshold: Option<u64>,

    /// Environment variables for the canister as key-value pairs.
    /// These variables are accessible within the canister and can be used to configure
    /// behavior without hardcoding values in the WASM module.
    pub environment_variables: Option<HashMap<String, String>>,
}

impl From<Settings> for CanisterSettingsArg {
    fn from(settings: Settings) -> Self {
        CanisterSettingsArg {
            freezing_threshold: settings.freezing_threshold.map(Nat::from),
            controllers: None,
            reserved_cycles_limit: settings.reserved_cycles_limit.map(Nat::from),
            log_visibility: settings.log_visibility.map(Into::into),
            memory_allocation: settings.memory_allocation.map(Nat::from),
            compute_allocation: settings.compute_allocation.map(Nat::from),
        }
    }
}
