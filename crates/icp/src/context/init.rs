use std::{env::current_dir, sync::Arc};

use console::Term;
use snafu::prelude::*;

use crate::canister::build::Builder;
use crate::canister::recipe::handlebars::Handlebars;
use crate::canister::sync::Syncer;
use crate::context::Context;
use crate::directories::{Access as _, Directories};
use crate::store_artifact::ArtifactStore;
use crate::{
    Lazy, Loader, ProjectLoaders, agent, canister, identity, manifest, network, prelude::*,
    project, store_id,
};

#[derive(Debug, Snafu)]
pub enum ContextInitError {
    #[snafu(display("failed to initialize directories"))]
    Directories {
        source: crate::directories::DirectoriesError,
    },

    #[snafu(display("failed to get current working directory"))]
    Cwd { source: std::io::Error },

    #[snafu(display("failed to convert path to UTF-8"))]
    Utf8Path { source: FromPathBufError },

    #[snafu(display("failed to lock identity directory"))]
    IdentityDirectory { source: crate::fs::lock::LockError },
}

pub fn initialize(
    project_root_override: Option<PathBuf>,
    term: Term,
    debug: bool,
) -> Result<Context, ContextInitError> {
    // Setup global directory structure
    let dirs = Arc::new(Directories::new().context(DirectoriesSnafu)?);

    // Project Root
    let project_root_locate = Arc::new(manifest::ProjectRootLocateImpl::new(
        current_dir()
            .context(CwdSnafu)?
            .try_into()
            .context(Utf8PathSnafu)?, // cwd
        project_root_override, // dir
    ));

    // Canister ID Store
    let ids = Arc::new(store_id::AccessImpl::new(project_root_locate.clone()));

    // Canister Artifact Store (wasm)
    let artifacts = Arc::new(ArtifactStore::new(project_root_locate.clone()));

    // Prepare http client
    let http_client = reqwest::Client::new();

    // Recipes
    let recipe = Arc::new(Handlebars { http_client });

    // Canister loader
    let cload = Arc::new(canister::PathLoader);

    // Canister builder
    let builder = Arc::new(Builder);

    // Canister syncer
    let syncer = Arc::new(Syncer);

    // Project Loaders
    let ploaders = ProjectLoaders {
        path: Arc::new(project::PathLoader),
        manifest: Arc::new(project::ManifestLoader {
            project_root_locate: project_root_locate.clone(),
            recipe,
            canister: cload,
        }),
    };

    let pload = Loader {
        project_root_locate: project_root_locate.clone(),
        project: ploaders,
    };

    let pload = Lazy::new(pload);
    let pload = Arc::new(pload);

    // Identity loader
    let idload = Arc::new(identity::Loader {
        dir: dirs.identity().context(IdentityDirectorySnafu)?,
    });

    // Network accessor
    let netaccess = Arc::new(network::Accessor {
        project_root_locate: project_root_locate.clone(),
        descriptors: dirs.port_descriptor(),
    });

    // Agent creator
    let agent_creator = Arc::new(agent::Creator);

    // Setup environment
    Ok(Context {
        term,
        dirs,
        ids,
        artifacts,
        project: pload,
        identity: idload,
        network: netaccess,
        agent: agent_creator,
        builder,
        syncer,
        debug,
    })
}
