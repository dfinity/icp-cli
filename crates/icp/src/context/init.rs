use std::{env::current_dir, sync::Arc};

use crate::canister::assets::Assets;
use crate::canister::build::Builder;
use crate::canister::prebuilt::Prebuilt;
use crate::canister::recipe::handlebars::Handlebars;
use crate::canister::script::Script;
use crate::canister::sync::Syncer;
use crate::directories::{Access as _, Directories};
use crate::{
    Lazy, Loader, ProjectLoaders, agent, canister, identity, manifest, network, prelude::*,
    project, store_id,
};
use anyhow::Error;
use console::Term;

use crate::context::Context;
use crate::store_artifact::ArtifactStore;

pub fn initialize(
    project_root_override: Option<PathBuf>,
    term: Term,
    debug: bool,
) -> Result<Context, Error> {
    // Setup global directory structure
    let dirs = Arc::new(Directories::new()?);

    // Project Root
    let project_root_locate = Arc::new(manifest::ProjectRootLocateImpl::new(
        current_dir()?.try_into()?, // cwd
        project_root_override,      // dir
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

    // Builders/Syncers
    let cprebuilt = Arc::new(Prebuilt);
    let cassets = Arc::new(Assets);
    let cscript = Arc::new(Script);

    // Canister builder
    let builder = Arc::new(Builder {
        prebuilt: cprebuilt.to_owned(),
        script: cscript.to_owned(),
    });

    // Canister syncer
    let syncer = Arc::new(Syncer {
        assets: cassets.to_owned(),
        script: cscript.to_owned(),
    });

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
        dir: dirs.identity()?,
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
