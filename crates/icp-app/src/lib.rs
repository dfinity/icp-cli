//! Everything about the machine the tool is running on, rather than about a
//! project.
//!
//! Identities and the keyring, user settings, the global directory layout, the
//! package cache, local networks and the launcher that runs them, telemetry,
//! and the operations that act on a canister by principal rather than by what
//! some manifest says about it.
//!
//! Projects — manifests, building, installing, syncing, deploying — are
//! [`icp`], which this crate depends on and which does not depend on this one.
//! The seams that project code reaches the machine through
//! ([`icp::network::Access`], [`icp::canister::wasm::Fetch`],
//! [`icp::canister::recipe::Resolve`], [`icp::host::Observe`]) are declared
//! there and implemented here.

pub mod agent;
pub mod context;
pub mod directories;
pub mod identity;
pub mod network;
pub mod operations;
pub mod package;
pub mod recipe;
pub mod settings;
pub mod signed_message;
pub mod telemetry_data;
pub mod wasm;
