use std::{sync::Arc, time::Duration};

use snafu::prelude::*;

use crate::canister::build::Builder;
use crate::canister::sync::Syncer;
use crate::context::Context;
use crate::store_artifact::ArtifactStore;

use crate::{
    ProjectLoad, agent, identity, identity::PasswordFunc, manifest::ProjectRootLocate, network,
    store_id,
};

#[derive(Debug, Snafu)]
pub enum ContextInitError {
    #[snafu(display("failed to lock identity directory"))]
    IdentityDirectory { source: crate::fs::lock::LockError },
}

/// Assembles the library context from the ports the host provides: where its
/// data lives, how to find a project, and how to load one.
pub fn initialize(
    dirs: Arc<dyn crate::directories::Access>,
    project_root_locate: Arc<dyn ProjectRootLocate>,
    project: Arc<dyn ProjectLoad>,
    password_func: PasswordFunc,
    pem_session_duration: Option<Duration>,
) -> Result<Context, ContextInitError> {
    // Canister ID Store
    let ids = Arc::new(store_id::AccessImpl::new(project_root_locate.clone()));

    // Canister Artifact Store (wasm)
    let artifacts = Arc::new(ArtifactStore::new(project_root_locate.clone()));

    // Canister builder
    let builder = Arc::new(Builder);

    // Canister syncer
    let syncer = Arc::new(Syncer);

    // Telemetry data bag (written by subsystems, read at session finish)
    let telemetry_data = Arc::new(crate::telemetry_data::TelemetryData::default());

    // Identity loader
    let idload = Arc::new(identity::Loader::new(
        dirs.identity().context(IdentityDirectorySnafu)?,
        password_func.clone(),
        pem_session_duration,
        telemetry_data.clone(),
    ));

    if let Ok(mockdir) = std::env::var("ICP_CLI_KEYRING_MOCK_DIR") {
        keyring::set_default_credential_builder(Box::new(
            crate::identity::keyring_mock::MockKeyring {
                dir: crate::prelude::PathBuf::from(mockdir),
            },
        ));
    }

    // Agent creator
    let agent_creator = Arc::new(agent::Creator);

    // Network accessor
    let netaccess = Arc::new(network::Accessor {
        project_root_locate,
        descriptors: dirs.port_descriptor(),
        agent: agent_creator.clone(),
    });

    // Setup environment
    Ok(Context {
        dirs,
        ids,
        artifacts,
        project,
        identity: idload,
        network: netaccess,
        agent: agent_creator,
        builder,
        syncer,
        telemetry_data,
        password_func,
    })
}
