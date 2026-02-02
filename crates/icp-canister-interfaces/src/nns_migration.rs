use candid::{CandidType, Deserialize, Principal, Reserved};
use std::fmt;

/// The NNS migration canister ID.
pub const NNS_MIGRATION_CID: &str = "sbzkb-zqaaa-aaaaa-aaaiq-cai";

/// The NNS migration canister principal.
pub const NNS_MIGRATION_PRINCIPAL: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11, 0x01, 0x01]);

/// Arguments for the `migrate_canister` method.
#[derive(Clone, CandidType, Deserialize, Debug, PartialEq, Eq)]
pub struct MigrateCanisterArgs {
    pub migrated_canister_id: Principal,
    pub replaced_canister_id: Principal,
}

/// Validation errors returned by the NNS migration canister.
#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum ValidationError {
    MigrationsDisabled(Reserved),
    RateLimited(Reserved),
    ValidationInProgress { canister: Principal },
    MigrationInProgress { canister: Principal },
    CanisterNotFound { canister: Principal },
    SameSubnet(Reserved),
    CallerNotController { canister: Principal },
    NotController { canister: Principal },
    MigratedCanisterNotStopped(Reserved),
    MigratedCanisterNotReady(Reserved),
    ReplacedCanisterNotStopped(Reserved),
    ReplacedCanisterHasSnapshots(Reserved),
    MigratedCanisterInsufficientCycles(Reserved),
    CallFailed { reason: String },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::MigrationsDisabled(_) => {
                write!(f, "Canister migrations are disabled at the moment")
            }
            ValidationError::RateLimited(_) => {
                write!(
                    f,
                    "Canister migration has been rate-limited. Try again later"
                )
            }
            ValidationError::ValidationInProgress { canister } => {
                write!(
                    f,
                    "Validation for canister {canister} is already in progress"
                )
            }
            ValidationError::MigrationInProgress { canister } => {
                write!(
                    f,
                    "Canister migration for canister {canister} is already in progress"
                )
            }
            ValidationError::CanisterNotFound { canister } => {
                write!(f, "The canister {canister} does not exist")
            }
            ValidationError::SameSubnet(_) => {
                write!(f, "Both canisters are on the same subnet")
            }
            ValidationError::CallerNotController { canister } => {
                write!(
                    f,
                    "The canister {canister} is not controlled by the calling identity"
                )
            }
            ValidationError::NotController { canister } => {
                write!(
                    f,
                    "The NNS migration canister ({NNS_MIGRATION_PRINCIPAL}) is not a controller of canister {canister}"
                )
            }
            ValidationError::MigratedCanisterNotStopped(_) => {
                write!(f, "The migrated canister is not stopped")
            }
            ValidationError::MigratedCanisterNotReady(_) => {
                write!(
                    f,
                    "The migrated canister is not ready for migration. Try again later"
                )
            }
            ValidationError::ReplacedCanisterNotStopped(_) => {
                write!(f, "The replaced canister is not stopped")
            }
            ValidationError::ReplacedCanisterHasSnapshots(_) => {
                write!(f, "The replaced canister has snapshots")
            }
            ValidationError::MigratedCanisterInsufficientCycles(_) => {
                write!(
                    f,
                    "The migrated canister does not have enough cycles for migration. Top up with at least 10T cycles"
                )
            }
            ValidationError::CallFailed { reason } => {
                write!(f, "Internal IC error: a call failed due to {reason}")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Result type for the `migrate_canister` method.
pub type MigrateCanisterResult = Result<(), Option<ValidationError>>;

/// Migration status returned by the `migration_status` query.
#[derive(Clone, CandidType, Deserialize, Debug, PartialEq, Eq)]
pub enum MigrationStatus {
    InProgress { status: String },
    Failed { reason: String, time: u64 },
    Succeeded { time: u64 },
}

impl fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationStatus::InProgress { status } => write!(f, "In progress: {status}"),
            MigrationStatus::Failed { reason, .. } => write!(f, "Failed: {reason}"),
            MigrationStatus::Succeeded { .. } => write!(f, "Succeeded"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nns_migration_cid_and_principal_match() {
        assert_eq!(NNS_MIGRATION_CID, NNS_MIGRATION_PRINCIPAL.to_text());
    }
}
