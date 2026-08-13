use std::sync::Arc;

use crate::canister::build::Builder;
use crate::canister::sync::Syncer;
use crate::context::Context;
use crate::store_artifact::ArtifactStore;

use crate::{ProjectLoad, agent, manifest::ProjectRootLocate, network, store_id};

/// Assembles the library context from the ports the host provides: where its
/// data lives, how to find a project, and how to load one.
pub fn initialize(
    dirs: Arc<dyn crate::directories::Access>,
    project_root_locate: Arc<dyn ProjectRootLocate>,
    project: Arc<dyn ProjectLoad>,
) -> Context {
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

    // Agent creator
    let agent_creator = Arc::new(agent::Creator);

    // Network accessor
    let netaccess = Arc::new(network::Accessor {
        project_root_locate,
        descriptors: dirs.port_descriptor(),
        agent: agent_creator.clone(),
    });

    Context {
        dirs,
        ids,
        artifacts,
        project,
        network: netaccess,
        agent: agent_creator,
        builder,
        syncer,
        telemetry_data,
    }
}
