use backoff::{ExponentialBackoff, backoff::Backoff};
use candid::Principal;
use ic_agent::{Agent, AgentError};
use ic_utils::Canister;
use icp_canister_interfaces::nns_migration::{
    MigrateCanisterArgs, MigrateCanisterResult, MigrationStatus, NNS_MIGRATION_PRINCIPAL,
    ValidationError,
};
use icp_canister_interfaces::registry::{
    GetSubnetForCanisterRequest, GetSubnetForCanisterResult, GetSubnetForCanisterSuccess,
    REGISTRY_PRINCIPAL,
};
use snafu::{ResultExt, Snafu};

const MIGRATE_CANISTER_METHOD: &str = "migrate_canister";
const MIGRATION_STATUS_METHOD: &str = "migration_status";
const GET_SUBNET_FOR_CANISTER_METHOD: &str = "get_subnet_for_canister";

#[derive(Debug, Snafu)]
pub enum CanisterMigrationError {
    #[snafu(display("Failed to call NNS migration canister"))]
    CallMigrationCanister { source: AgentError },

    #[snafu(display("Failed to query migration status"))]
    QueryMigrationStatus { source: AgentError },

    #[snafu(display("Validation failed: {source}"))]
    ValidationFailed { source: ValidationError },

    #[snafu(display("Validation failed with unknown error"))]
    ValidationFailedUnknown,

    #[snafu(display("Failed to query registry canister"))]
    QueryRegistryCanister { source: AgentError },

    #[snafu(display("Failed to determine subnet for canister {canister_id}: {reason}"))]
    SubnetLookupFailed {
        canister_id: Principal,
        reason: String,
    },
}

/// Initiate a canister ID migration via the NNS migration canister.
///
/// This transfers the canister ID from `migrated_canister` to the subnet where
/// `replaced_canister` resides. The `replaced_canister` will be deleted and its
/// canister ID taken over by the migrated canister.
///
/// Prerequisites:
/// - Both canisters must be stopped
/// - The NNS migration canister must be a controller of both canisters
/// - The canisters must be on different subnets
/// - The migrated canister must have at least 10T cycles
/// - The replaced canister must have no snapshots
pub async fn migrate_canister(
    agent: &Agent,
    migrated_canister: Principal,
    replaced_canister: Principal,
) -> Result<(), CanisterMigrationError> {
    let canister = Canister::builder()
        .with_agent(agent)
        .with_canister_id(NNS_MIGRATION_PRINCIPAL)
        .build()
        .expect("failed to build canister");

    let arg = MigrateCanisterArgs {
        migrated_canister_id: migrated_canister,
        replaced_canister_id: replaced_canister,
    };

    let (result,): (MigrateCanisterResult,) = canister
        .update(MIGRATE_CANISTER_METHOD)
        .with_arg(arg)
        .build()
        .await
        .context(CallMigrationCanisterSnafu)?;

    match result {
        Ok(()) => Ok(()),
        Err(None) => Err(CanisterMigrationError::ValidationFailedUnknown),
        Err(Some(err)) => Err(CanisterMigrationError::ValidationFailed { source: err }),
    }
}

/// Query the migration status for a canister migration.
pub async fn migration_status(
    agent: &Agent,
    migrated_canister: Principal,
    replaced_canister: Principal,
) -> Result<Option<MigrationStatus>, CanisterMigrationError> {
    let canister = Canister::builder()
        .with_agent(agent)
        .with_canister_id(NNS_MIGRATION_PRINCIPAL)
        .build()
        .expect("failed to build canister");

    let arg = MigrateCanisterArgs {
        migrated_canister_id: migrated_canister,
        replaced_canister_id: replaced_canister,
    };

    let (result,): (Option<MigrationStatus>,) = canister
        .query(MIGRATION_STATUS_METHOD)
        .with_arg(arg)
        .build()
        .await
        .context(QueryMigrationStatusSnafu)?;

    Ok(result)
}

/// Get the subnet ID for a canister by querying the registry canister.
pub async fn get_subnet_for_canister(
    agent: &Agent,
    canister_id: Principal,
) -> Result<Principal, CanisterMigrationError> {
    let registry_canister = Canister::builder()
        .with_agent(agent)
        .with_canister_id(REGISTRY_PRINCIPAL)
        .build()
        .expect("failed to build canister");

    let mut backoff = ExponentialBackoff::default();

    loop {
        let arg = GetSubnetForCanisterRequest {
            principal: Some(canister_id),
        };

        let result: Result<(GetSubnetForCanisterResult,), AgentError> = registry_canister
            .query(GET_SUBNET_FOR_CANISTER_METHOD)
            .with_arg(arg)
            .build()
            .await;

        match result {
            Ok((Ok(GetSubnetForCanisterSuccess {
                subnet_id: Some(subnet_id),
            }),)) => return Ok(subnet_id),
            Ok((Ok(GetSubnetForCanisterSuccess { subnet_id: None }),)) => {
                return Err(CanisterMigrationError::SubnetLookupFailed {
                    canister_id,
                    reason: "no subnet found".to_string(),
                });
            }
            Ok((Err(text),)) => {
                return Err(CanisterMigrationError::SubnetLookupFailed {
                    canister_id,
                    reason: text,
                });
            }
            Err(agent_err) if is_retryable(&agent_err) => {
                if let Some(duration) = backoff.next_backoff() {
                    tokio::time::sleep(duration).await;
                    continue;
                }
                return Err(CanisterMigrationError::QueryRegistryCanister { source: agent_err });
            }
            Err(agent_err) => {
                return Err(CanisterMigrationError::QueryRegistryCanister { source: agent_err });
            }
        }
    }
}

fn is_retryable(error: &AgentError) -> bool {
    matches!(
        error,
        AgentError::TimeoutWaitingForResponse() | AgentError::TransportError(_)
    )
}
