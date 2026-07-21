//! Abstracted canister-id store.

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
/// The `is_cache` flag selects the managed-network cache store vs. the
/// connected-network data store, matching the host implementation. Passed as
/// `&dyn IdStore` (interior-mutable), not `&mut`.
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
