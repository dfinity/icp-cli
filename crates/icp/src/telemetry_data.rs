//! Lightweight data bag for telemetry values discovered during command execution.
//!
//! Subsystems (e.g. the identity loader) write to [`TelemetryData`] via
//! interior mutability. The CLI telemetry session reads it at finish time
//! to build the final record.

use std::sync::Mutex;

use serde::Serialize;

use crate::identity::manifest::IdentitySpec;

/// Data collected during command execution for telemetry.
///
/// Stored in [`crate::context::Context`] so any subsystem with access to
/// the context can record values. All fields use interior mutability so
/// only a shared reference (`&self`) is required.
#[derive(Default)]
pub struct TelemetryData {
    identity_type: Mutex<Option<IdentityStorageType>>,
}

impl TelemetryData {
    pub fn set_identity_type(&self, t: IdentityStorageType) {
        *self.identity_type.lock().unwrap() = Some(t);
    }

    pub fn identity_type(&self) -> Option<IdentityStorageType> {
        *self.identity_type.lock().unwrap()
    }
}

/// How an identity's private key is stored.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityStorageType {
    Pem,
    Keyring,
    Hsm,
    Anonymous,
}

impl From<&IdentitySpec> for IdentityStorageType {
    fn from(spec: &IdentitySpec) -> Self {
        match spec {
            IdentitySpec::Pem { .. } => Self::Pem,
            IdentitySpec::Keyring { .. } => Self::Keyring,
            IdentitySpec::Hsm { .. } => Self::Hsm,
            IdentitySpec::Anonymous => Self::Anonymous,
        }
    }
}
