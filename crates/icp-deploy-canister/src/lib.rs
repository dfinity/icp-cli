//! Canister installation, syncing, and the project model, with all host IO
//! abstracted behind trait objects so the core can run inside a canister.
//!
//! See the module-level docs on the IO traits ([`files`], [`icp_access`],
//! [`ids`]) for the abstraction boundary.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use serde::Serialize;
use snafu::prelude::*;

use candid_parser::parse_idl_args;

use crate::{
    canister::Settings,
    manifest::{
        ArgsFormat,
        canister::{BuildSteps, SyncSteps},
    },
    network::Configuration,
    prelude::*,
};

pub mod canister;
pub mod deploy;
pub mod fs;
pub mod ids;
pub mod manifest;
pub mod network;
pub mod parsers;
pub mod prelude;
pub mod project;
pub mod sync_exec;

pub use deploy::{
    DeployCanisterError, DeployError, InstallCanisterError, InstallMode, SyncCanisterError,
    SyncStepError, UpdateOrProxyError, apply_binding_env_vars, binding_env_vars, deploy,
    deploy_canister, install_canister, install_canister_resolved, resolve_install_mode_and_status,
    run_sync_steps, start_canister, sync_canister,
};
pub use ids::{IdMapping, IdStore, IdStoreError};
pub use project::{consolidate_manifest, load_project, verify_sandbox};
pub use sync_exec::{
    ScriptInvocation, ScriptRunError, ScriptRunner, StepProgress, SyncStepContext, system_env_vars,
};

/// Resolved initialization arguments, with any file references already loaded.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum InitArgs {
    /// Text content (inline or loaded from file). Format is always known.
    Text { content: String, format: ArgsFormat },
    /// Raw binary bytes (from a file with `format: bin`). Used directly.
    Binary(Vec<u8>),
}

#[derive(Debug, Snafu)]
pub enum InitArgsToBytesError {
    #[snafu(display("failed to decode hex init args"))]
    HexDecode { source: hex::FromHexError },

    #[snafu(display("failed to parse Candid init args"))]
    CandidParse { source: candid_parser::Error },

    #[snafu(display("failed to encode Candid init args to bytes"))]
    CandidEncode { source: candid::Error },
}

impl InitArgs {
    /// Resolve to raw bytes according to the format.
    pub fn to_bytes(&self) -> Result<Vec<u8>, InitArgsToBytesError> {
        match self {
            InitArgs::Binary(bytes) => Ok(bytes.clone()),
            InitArgs::Text { content, format } => match format {
                ArgsFormat::Hex => hex::decode(content.trim()).context(HexDecodeSnafu),
                ArgsFormat::Candid => {
                    let args = parse_idl_args(content.trim()).context(CandidParseSnafu)?;
                    args.to_bytes().context(CandidEncodeSnafu)
                }
                ArgsFormat::Bin => {
                    unreachable!("binary format cannot appear in InitArgs::Text")
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Canister {
    pub name: String,

    /// Canister settings, such as memory constaints, etc.
    pub settings: Settings,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    pub build: BuildSteps,

    /// The configuration specifying how to sync the canister
    pub sync: SyncSteps,

    /// Initialization arguments passed to the canister during installation.
    /// Resolved from the manifest — file contents are already loaded.
    pub init_args: Option<InitArgs>,

    /// If the canister was defined via a recipe reference, this holds the
    /// original recipe specifier string (e.g. `@dfinity/motoko@v4.0.0`).
    /// `None` when the canister uses explicit build/sync instructions.
    pub registry_recipe: Option<String>,

    /// Canister-discovery wiring. Maps the name this canister reads in a
    /// `PUBLIC_CANISTER_ID:<name>` environment variable to the store key of the
    /// referenced canister. Computed during consolidation so each canister sees
    /// the view its owning project expects: its own project's canisters under
    /// their local names, plus any declared dependencies under their aliases
    /// (`<alias>:<canister>`). For a project with no dependencies this maps every
    /// canister's local name to itself, reproducing the flat "every canister sees
    /// every sibling" behavior.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,

    /// Subdomain prefixes for the canister's friendly URLs, most-specific label
    /// first, e.g. `["backend"]` for an own canister or `["backend.openemail"]`
    /// for a dependency canister (dot-nested by alias chain). A de-duplicated
    /// shared dependency canister carries one entry per alias chain that reaches
    /// it. Consumed only at deploy time to build `custom-domains.txt` entries and
    /// the printed URLs; a runtime display aid that is always recomputed during
    /// consolidation, so it is never serialized.
    #[serde(skip)]
    pub friendly_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Network {
    pub name: String,
    pub configuration: Configuration,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Environment {
    pub name: String,
    pub network: Network,
    pub canisters: IndexMap<String, (PathBuf, Canister)>,
}

impl Environment {
    pub fn get_canister_names(&self) -> Vec<String> {
        self.canisters.keys().cloned().collect()
    }

    pub fn contains_canister(&self, canister_name: &str) -> bool {
        self.canisters.contains_key(canister_name)
    }

    pub fn get_canister_info(&self, canister: &str) -> Result<(PathBuf, Canister), String> {
        self.canisters
            .get(canister)
            .ok_or_else(|| {
                format!(
                    "canister '{}' not declared in environment '{}'",
                    canister, self.name
                )
            })
            .cloned()
    }
}

/// Consolidated project definition
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Project {
    pub dir: PathBuf,
    pub canisters: IndexMap<String, (PathBuf, Canister)>,
    pub networks: HashMap<String, Network>,
    pub environments: HashMap<String, Environment>,

    /// Environments the workspace defines that some vendored member does *not*
    /// declare, keyed by environment name → the missing members' store-key
    /// prefixes. Enforced when the environment is selected (strict rule).
    /// Empty for standalone projects and workspaces whose members are complete.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub member_missing_envs: HashMap<String, Vec<String>>,
}

impl Project {
    pub fn get_canister(&self, canister_name: &str) -> Option<&(PathBuf, Canister)> {
        self.canisters.get(canister_name)
    }
}
