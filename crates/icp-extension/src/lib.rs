//! The host side of icp-cli extensions: a wasm component implementing the
//! `icp:extension` world (`extension.wit`), loaded into a wasmtime sandbox
//! with no filesystem, environment, sockets or processes.
//!
//! An extension *declares* its commands ([`Info`]); icp-cli parses them with
//! its own clap and hands the extension the result ([`Matched`]) together
//! with what it resolved for the command ([`RunContext`]). The extension's
//! only effects are the imports in [`RunHost`].

mod runtime;
mod validate;

use serde::{Deserialize, Serialize};

pub use runtime::{DescribeError, RunError, Terminal, describe, run};
pub use validate::{InvalidInfo, RESERVED_LONGS, RESERVED_SHORTS, validate};

/// Which flag set icp-cli attaches to a command. See `standard-opt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StandardOpt {
    Identity,
    Network,
}

/// How many values an argument takes. See `arg-kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArgKind {
    Flag,
    Value,
    Values,
}

/// One argument of a declared command. See `arg`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arg {
    pub name: String,
    pub long: Option<String>,
    pub short: Option<char>,
    pub kind: ArgKind,
    pub required: bool,
    pub default_value: Option<String>,
    pub possible_values: Vec<String>,
    pub value_name: Option<String>,
    pub help: String,
}

impl Arg {
    pub fn is_positional(&self) -> bool {
        self.long.is_none() && self.short.is_none()
    }
}

/// A declared command. See `command`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub path: Vec<String>,
    pub about: String,
    pub args: Vec<Arg>,
    pub standard: Vec<StandardOpt>,
    pub default_network: Option<String>,
}

/// A mainnet canister the extension may call. See `allowed-canister`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowedCanister {
    pub id: candid::Principal,
    pub label: String,
}

/// What an extension is: read once at install and kept beside it, so that
/// its commands can be offered without loading it. See `info`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub commands: Vec<Command>,
    pub canisters: Vec<AllowedCanister>,
}

/// The command the user invoked, as icp-cli parsed it. See `matched`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matched {
    pub path: Vec<String>,
    /// Each argument that was given or has a default, by name. A set flag has
    /// no values.
    pub values: Vec<(String, Vec<String>)>,
}

/// What icp-cli resolved for the invoked command. See `context`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunContext {
    pub identity_principal: Option<candid::Principal>,
    pub network: Option<String>,
    pub mainnet: bool,
    pub environment: Option<String>,
    pub canister_ids: Vec<(String, candid::Principal)>,
    pub interactive: bool,
}
