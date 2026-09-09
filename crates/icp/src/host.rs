//! What an operation needs from the world around it.
//!
//! Operations act on a project: they load it, resolve an environment out of it,
//! look canister ids up and record new ones, build and sync canisters. Every
//! one of those is reached through a trait object here rather than through the
//! ambient [`Context`](crate::context::Context), which also carries the
//! identity loader, the keyring-backed key store and the global directories —
//! none of which an operation has any business reaching.
//!
//! So this is the whole surface. An operation takes `&Host` and whatever its
//! caller resolved for it (an agent, install arguments), and nothing else.

use std::sync::Arc;

use candid::Principal;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::{
    Canister,
    canister::{build::Build, sync::Synchronize},
    network::{Configuration as NetworkConfiguration, FriendlyDomains},
    prelude::*,
    store_id::{IdMapping, LookupIdError},
};

/// Selection type for environments
#[derive(Clone, Debug, PartialEq)]
pub enum EnvironmentSelection {
    /// Use the default environment (local)
    Default,
    /// Use a named environment
    Named(String),
}

impl EnvironmentSelection {
    pub fn name(&self) -> &str {
        match self {
            EnvironmentSelection::Default => LOCAL,
            EnvironmentSelection::Named(name) => name,
        }
    }
}

/// Selection type for canisters
#[derive(Clone, Debug, PartialEq)]
pub enum CanisterSelection {
    /// Use a canister by name (requires project context)
    Named(String),
    /// Use a canister by principal
    Principal(Principal),
}

/// The project-side resources an operation runs against.
#[derive(Clone)]
pub struct Host {
    /// Project loader
    pub project: Arc<dyn crate::ProjectLoad>,

    /// Canister ID store for lookup and storage
    pub ids: Arc<dyn crate::store_id::Access>,

    /// Store for canister build artifacts
    pub artifacts: Arc<dyn crate::store_artifact::Access>,

    /// Canister builder
    pub builder: Arc<dyn Build>,

    /// Canister synchronizer
    pub syncer: Arc<dyn Synchronize>,

    /// Source of wasm modules a manifest names but the project does not contain
    pub wasm: Arc<dyn crate::canister::wasm::Fetch>,

    /// Network resolution: endpoints, root keys, friendly domains
    pub network: Arc<dyn crate::network::Access>,

    /// Where to report what resolution turned up. See [`Observe`].
    pub observer: Arc<dyn Observe>,
}

/// Somewhere for the surrounding application to notice what resolution turned
/// up.
///
/// Which project is loaded, and which environment was picked out of it, are
/// facts an application wants — for telemetry, for a status line — and they are
/// established in here, part-way through resolving something else, not at the
/// call site. Rather than let an app-scoped collector be written to from this
/// layer, the facts are handed over and what becomes of them is not this
/// layer's concern.
pub trait Observe: Send + Sync {
    /// An environment was resolved out of a loaded project.
    ///
    /// Called on every resolution, not just the first, so implementations must
    /// tolerate being told the same thing repeatedly.
    fn environment_resolved(&self, _project: &crate::Project, _environment: &crate::Environment) {}
}

/// The [`Observe`] for a caller that does not care.
pub struct Ignore;

impl Observe for Ignore {}

impl Host {
    #[cfg(any(test, feature = "test-util"))]
    /// A host whose every seam is a mock, for tests that only exercise the
    /// resolution methods below.
    pub fn mocked() -> Self {
        Self {
            project: Arc::new(crate::MockProjectLoader::minimal()),
            ids: Arc::new(crate::store_id::mock::MockInMemoryIdStore::new()),
            artifacts: Arc::new(crate::store_artifact::MockInMemoryArtifactStore::new()),
            builder: Arc::new(crate::canister::build::UnimplementedMockBuilder),
            syncer: Arc::new(crate::canister::sync::UnimplementedMockSyncer),
            wasm: Arc::new(crate::canister::wasm::UnimplementedMockFetch),
            network: Arc::new(crate::network::MockNetworkAccessor::new()),
            observer: Arc::new(Ignore),
        }
    }

    /// Gets an environment by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the environment is not found.
    pub async fn get_environment(
        &self,
        environment: &EnvironmentSelection,
    ) -> Result<crate::Environment, GetEnvironmentError> {
        // Load project
        let p = self.project.load().await?;

        // Load target environment
        let env = p
            .environments
            .get(environment.name())
            .context(EnvironmentNotFoundSnafu {
                name: environment.name().to_owned(),
            })?;

        // Strict rule: every vendored member must declare the selected
        // environment. Enforced here (not at load time) so a member missing some
        // other environment never blocks deploys to the ones it does declare.
        if let Some(missing) = p.member_missing_envs.get(environment.name())
            && let Some(member) = missing.first()
        {
            return MissingDependencyEnvironmentSnafu {
                environment: environment.name().to_owned(),
                member: member.clone(),
            }
            .fail();
        }

        self.observer.environment_resolved(&p, env);

        Ok(env.clone())
    }

    pub async fn get_canister_and_path_for_env(
        &self,
        canister_name: &str,
        environment: &EnvironmentSelection,
    ) -> Result<(PathBuf, Canister), GetEnvCanisterError> {
        let p = self.project.load().await?;
        let Some((path, canister)) = p.get_canister(canister_name) else {
            return CanisterNotFoundInProjectSnafu {
                canister_name: canister_name.to_owned(),
            }
            .fail();
        };

        let env = self.get_environment(environment).await?;
        if !env.contains_canister(canister_name) {
            return CanisterNotInEnvSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            }
            .fail();
        }
        Ok((path.clone(), canister.clone()))
    }

    /// Gets the canister ID for a given canister selection in a specified environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment cannot be loaded or if the canister ID cannot be found.
    pub async fn get_canister_id_for_env(
        &self,
        canister: &CanisterSelection,
        environment: &EnvironmentSelection,
    ) -> Result<Principal, GetCanisterIdForEnvError> {
        let principal = match canister {
            CanisterSelection::Named(canister_name) => {
                let env = self.get_environment(environment).await?;
                let is_cache = match env.network.configuration {
                    NetworkConfiguration::Managed { .. } => true,
                    NetworkConfiguration::Connected { .. } => false,
                };

                if !env.canisters.contains_key(canister_name) {
                    return CanisterNotFoundInEnvSnafu {
                        canister_name: canister_name.to_owned(),
                        environment_name: environment.name().to_owned(),
                    }
                    .fail();
                }

                // Lookup the canister id
                self.ids
                    .lookup(is_cache, &env.name, canister_name)
                    .context(CanisterIdLookupSnafu {
                        canister_name: canister_name.to_owned(),
                        environment_name: environment.name().to_owned(),
                    })?
            }
            CanisterSelection::Principal(principal) => {
                // Make sure a valid environment was requested
                let _ = self.get_environment(environment).await?;
                *principal
            }
        };

        Ok(principal)
    }

    /// Sets the canister ID for a given canister name in a specified environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment cannot be loaded or if the canister ID cannot be registered.
    pub async fn set_canister_id_for_env(
        &self,
        canister_name: &str,
        canister_id: Principal,
        environment: &EnvironmentSelection,
    ) -> Result<(), SetCanisterIdForEnvError> {
        let env = self.get_environment(environment).await?;
        let is_cache = match env.network.configuration {
            NetworkConfiguration::Managed { .. } => true,
            NetworkConfiguration::Connected { .. } => false,
        };

        if !env.canisters.contains_key(canister_name) {
            return SetCanisterNotFoundInEnvSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            }
            .fail();
        }

        // Register the canister id
        self.ids
            .register(is_cache, &env.name, canister_name, canister_id)
            .context(CanisterIdRegisterSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            })?;

        Ok(())
    }

    /// Removes the canister ID for a given canister name in a specified environment.
    pub async fn remove_canister_id_for_env(
        &self,
        canister_name: &str,
        environment: &EnvironmentSelection,
    ) -> Result<(), RemoveCanisterIdForEnvError> {
        let env = self.get_environment(environment).await?;
        let is_cache = match env.network.configuration {
            NetworkConfiguration::Managed { .. } => true,
            NetworkConfiguration::Connected { .. } => false,
        };

        // Unregister the canister id
        self.ids
            .unregister(is_cache, &env.name, canister_name)
            .context(CanisterIdUnregisterSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            })?;

        Ok(())
    }

    pub async fn ids_by_environment(
        &self,
        environment: &EnvironmentSelection,
    ) -> Result<IdMapping, GetIdsByEnvironmentError> {
        let env = self.get_environment(environment).await?;
        let is_cache = match env.network.configuration {
            NetworkConfiguration::Managed { .. } => true,
            NetworkConfiguration::Connected { .. } => false,
        };
        self.ids
            .lookup_by_environment(is_cache, environment.name())
            .context(IdsByEnvironmentLookupSnafu {
                environment_name: environment.name().to_owned(),
            })
    }

    /// Republishes the friendly canister domains of the managed network the
    /// given environment targets.
    ///
    /// Hands the network layer the means to collect the project's
    /// `friendly name -> canister id` mappings, rather than the mappings
    /// themselves: it owns where they are written and which network is actually
    /// being served, and only it knows whether there is anything to write at
    /// all — so it decides whether the id store is read.
    ///
    /// This is a best-effort operation: a failure to update friendly domains
    /// should not block canister creation or deletion, so nothing here is
    /// propagated.
    pub async fn update_custom_domains(&self, environment: &EnvironmentSelection) {
        let Ok(env) = self.get_environment(environment).await else {
            return;
        };
        let NetworkConfiguration::Managed { .. } = &env.network.configuration else {
            return;
        };
        // The load is cached, so this costs a clone; the per-environment id
        // lookups are what the callback defers.
        let Ok(project) = self.project.load().await else {
            return;
        };

        self.network
            .publish_friendly_domains(&env.network, &|network| {
                collect_friendly_domains(&project, &*self.ids, network)
            })
            .await;
    }
}

/// The friendly-name mappings of every environment targeting `network`.
///
/// Turns each environment's stored `store_key -> principal` mapping into
/// `(friendly_name, principal)` entries by joining against the consolidated
/// canisters (keyed by the same store key). A canister contributes one entry per
/// friendly name — several for a de-duplicated shared dependency canister.
fn collect_friendly_domains(
    project: &crate::Project,
    ids: &dyn crate::store_id::Access,
    network: &str,
) -> Vec<FriendlyDomains> {
    let mut collected = Vec::new();
    for (env_name, env) in &project.environments {
        if env.network.name != network {
            continue;
        }
        let is_cache = matches!(
            env.network.configuration,
            NetworkConfiguration::Managed { .. }
        );
        let Ok(mapping) = ids.lookup_by_environment(is_cache, env_name) else {
            continue;
        };
        let mut entries = Vec::new();
        for (store_key, principal) in &mapping {
            if let Some((_, canister)) = env.canisters.get(store_key) {
                for friendly_name in &canister.friendly_names {
                    entries.push((friendly_name.clone(), *principal));
                }
            }
        }
        if !entries.is_empty() {
            collected.push(FriendlyDomains {
                environment: env_name.clone(),
                entries,
            });
        }
    }
    collected
}

#[derive(Debug, Snafu)]
pub enum GetEnvironmentError {
    #[snafu(transparent)]
    ProjectLoad { source: crate::ProjectLoadError },

    #[snafu(display("project does not contain an environment named '{}'", name))]
    EnvironmentNotFound { name: String },

    #[snafu(display(
        "environment '{environment}' is not defined by dependency '{member}'; \
         a dependency must declare every environment the workspace targets"
    ))]
    MissingDependencyEnvironment { environment: String, member: String },
}

#[derive(Debug, Snafu)]
pub enum GetCanisterIdForEnvError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display(
        "canister '{}' not found in environment '{}'",
        canister_name,
        environment_name
    ))]
    CanisterNotFoundInEnv {
        canister_name: String,
        environment_name: String,
    },

    #[snafu(display(
        "failed to lookup canister ID for canister '{}' in environment '{}'",
        canister_name,
        environment_name
    ))]
    CanisterIdLookup {
        #[snafu(source(from(LookupIdError, Box::new)))]
        source: Box<LookupIdError>,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum SetCanisterIdForEnvError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display(
        "canister '{}' not found in environment '{}'",
        canister_name,
        environment_name
    ))]
    SetCanisterNotFoundInEnv {
        canister_name: String,
        environment_name: String,
    },

    #[snafu(display(
        "failed to register canister ID for canister '{}' in environment '{}'",
        canister_name,
        environment_name
    ))]
    CanisterIdRegister {
        source: crate::store_id::RegisterError,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum RemoveCanisterIdForEnvError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display(
        "failed to unregister canister ID for canister '{}' in environment '{}': {}",
        canister_name,
        environment_name,
        source
    ))]
    CanisterIdUnregister {
        source: crate::store_id::UnregisterError,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum GetIdsByEnvironmentError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display("failed to lookup IDs for environment '{environment_name}'"))]
    IdsByEnvironmentLookup {
        source: crate::store_id::LookupIdError,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum GetEnvCanisterError {
    #[snafu(transparent)]
    ProjectLoad { source: crate::ProjectLoadError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display("project does not contain a canister named '{canister_name}'"))]
    CanisterNotFoundInProject { canister_name: String },

    #[snafu(display(
        "environment '{environment_name}' does not contain a canister named '{canister_name}'"
    ))]
    CanisterNotInEnv {
        canister_name: String,
        environment_name: String,
    },
}
