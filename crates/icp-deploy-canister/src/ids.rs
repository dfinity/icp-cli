//! Canister-id store: per-environment `name → principal` mappings.

use std::collections::BTreeMap;

use candid::Principal;
use snafu::Snafu;

/// Mapping of canister names to their principals within an environment.
pub type IdMapping = BTreeMap<String, Principal>;

#[derive(Debug, Snafu)]
pub enum IdStoreError {
    #[snafu(display("could not find id for canister '{canister_name}' in environment '{env}'"))]
    NotFound { env: String, canister_name: String },

    #[snafu(display("failed to access canister id store for environment '{env}': {message}"))]
    Access { env: String, message: String },
}

/// Read/write access to canister-id mappings.
///
/// The `is_cache` flag lets an implementation that keeps two stores — a
/// managed-network cache and a connected-network data store — pick the right
/// one. `register` mutates through `&self`, so the store is interior-mutable.
pub trait IdStore: Send + Sync {
    fn lookup(
        &self,
        is_cache: bool,
        env: &str,
        canister_name: &str,
    ) -> Result<Principal, IdStoreError>;

    fn lookup_by_environment(&self, is_cache: bool, env: &str) -> Result<IdMapping, IdStoreError>;

    fn register(
        &self,
        is_cache: bool,
        env: &str,
        canister_name: &str,
        canister_id: Principal,
    ) -> Result<(), IdStoreError>;
}
