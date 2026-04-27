use std::{env::current_dir, sync::Arc};

use snafu::prelude::*;

use crate::canister::build::Builder;
use crate::canister::recipe::handlebars::Handlebars;
use crate::canister::sync::Syncer;
use crate::context::Context;
use crate::directories::{Access as _, Directories};
use crate::prelude::*;
use crate::store_artifact::ArtifactStore;
use crate::{
    Lazy, ProjectLoadImpl, agent, identity, identity::PasswordFunc, manifest, network, store_id,
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

    #[snafu(display("failed to lock package cache directory"))]
    PackageCache { source: crate::fs::lock::LockError },
}

pub fn initialize(
    project_root_override: Option<PathBuf>,
    debug: bool,
    password_func: PasswordFunc,
) -> Result<Context, ContextInitError> {
    // Setup global directory structure
    let dirs = Arc::new(Directories::new().context(DirectoriesSnafu)?);

    // Project Root. On Unix, prefer $PWD (the logical path the user cd'd
    // through) over getcwd(3), which resolves symlinks to the physical path
    // and would break upward traversal when the user is inside a symlinked
    // directory whose manifest sits above the symlink's location.
    #[cfg(unix)]
    let cwd: PathBuf = match std::env::var("PWD")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        Some(p) => p,
        None => PathBuf::try_from(current_dir().context(CwdSnafu)?).context(Utf8PathSnafu)?,
    };

    #[cfg(not(unix))]
    let cwd: PathBuf =
        PathBuf::try_from(current_dir().context(CwdSnafu)?).context(Utf8PathSnafu)?;

    let project_root_locate = Arc::new(manifest::ProjectRootLocateImpl::new(
        cwd,
        project_root_override,
    ));

    // Canister ID Store
    let ids = Arc::new(store_id::AccessImpl::new(project_root_locate.clone()));

    // Canister Artifact Store (wasm)
    let artifacts = Arc::new(ArtifactStore::new(project_root_locate.clone()));

    // Prepare http client
    let http_client = reqwest::Client::new();

    // Package cache
    let pkg_cache = dirs.package_cache().context(PackageCacheSnafu)?;

    // Recipes
    let recipe = Arc::new(Handlebars {
        http_client,
        pkg_cache,
    });

    // Canister builder
    let builder = Arc::new(Builder);

    // Canister syncer
    let syncer = Arc::new(Syncer);

    // Project loader
    let pload = ProjectLoadImpl {
        project_root_locate: project_root_locate.clone(),
        recipe,
    };

    let pload = Lazy::new(pload);
    let pload = Arc::new(pload);

    // Telemetry data bag (written by subsystems, read at session finish)
    let telemetry_data = Arc::new(crate::telemetry_data::TelemetryData::default());

    // Identity loader
    let idload = Arc::new(identity::Loader::new(
        dirs.identity().context(IdentityDirectorySnafu)?,
        password_func,
        telemetry_data.clone(),
    ));
    if let Ok(mockdir) = std::env::var("ICP_CLI_KEYRING_MOCK_DIR") {
        keyring::set_default_credential_builder(Box::new(
            crate::identity::keyring_mock::MockKeyring {
                dir: PathBuf::from(mockdir),
            },
        ));
    }

    // Network accessor
    let netaccess = Arc::new(network::Accessor {
        project_root_locate: project_root_locate.clone(),
        descriptors: dirs.port_descriptor(),
    });

    // Agent creator
    let agent_creator = Arc::new(agent::Creator);

    // Setup environment
    Ok(Context {
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
        telemetry_data,
    })
}
