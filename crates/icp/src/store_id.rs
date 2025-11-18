use std::collections::BTreeMap;
use std::sync::Arc;
use std::{io::ErrorKind, sync::Mutex};

use ic_agent::export::Principal;
use snafu::{ResultExt, Snafu};

use crate::{
    CACHE_DIR, DATA_DIR, ICP_BASE,
    fs::json,
    manifest::{ProjectRootLocate, ProjectRootLocateError},
    prelude::*,
};

/// Mapping of canister names to their Principals within an environment.
pub type IdMapping = BTreeMap<String, Principal>;

/// Loads the ID mapping from a given file path.
///
/// If the file does not exist, returns an empty mapping.
fn load_mapping(fpath: &Path) -> Result<IdMapping, json::Error> {
    json::load(fpath).or_else(|err| match err {
        // Default to empty
        json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(BTreeMap::new()),

        // Other
        _ => Err(err),
    })
}

/// Trait for accessing and managing canister ID mappings.
///
/// The mappings are stored at different places depending on whether the environment is on a managed or connected network.
/// For managed networks, the mappings are considered "cache".
/// For connected networks, the mappings are considered "data".
/// All the methods of this trait take an `is_cache` parameter to determine which store to use.
pub trait Access: Sync + Send {
    /// Register a mapping of (canister name, canister ID) for a given environment.
    fn register(
        &self,
        is_cache: bool,
        env: &str,
        canister_name: &str,
        canister_id: Principal,
    ) -> Result<(), RegisterError>;

    /// Lookup canister ID of a canister name in an environment.
    fn lookup(
        &self,
        is_cache: bool,
        env: &str,
        canister_name: &str,
    ) -> Result<Principal, LookupIdError>;

    /// Lookup all canister IDs for a given environment.
    fn lookup_by_environment(&self, is_cache: bool, env: &str) -> Result<IdMapping, LookupIdError>;
}

#[derive(Debug, Snafu)]
pub enum RegisterError {
    #[snafu(transparent)]
    ProjectRootLocate { source: ProjectRootLocateError },

    #[snafu(display("failed to load canister id store for environment '{env}'"))]
    RegisterLoadStore { source: json::Error, env: String },

    #[snafu(display(
        "canister '{canister_name}' in environment '{env}' is already registered with id '{id}'",
    ))]
    AlreadyRegistered {
        env: String,
        canister_name: String,
        id: Principal,
    },

    #[snafu(display("failed to save canister id mapping for environment '{env}'"))]
    RegisterSaveStore { source: json::Error, env: String },
}

#[derive(Debug, Snafu)]
pub enum LookupIdError {
    #[snafu(transparent)]
    ProjectRootLocate { source: ProjectRootLocateError },

    #[snafu(display("failed to load canister id store for environment '{env}'"))]
    LookupLoadStore { source: json::Error, env: String },

    #[snafu(display("could not find ID for canister '{canister_name}' in environment '{env}'"))]
    IdNotFound { env: String, canister_name: String },

    #[snafu(display("could not find canisters in environment '{}'", name))]
    EnvironmentNotFound { name: String },
}

/// Store of canister ID mappings for environments.
///
/// Each environment has a separate file storing its canister IDs mapping.
pub(crate) struct AccessImpl {
    project_root_locate: Arc<dyn ProjectRootLocate>,
    lock: Mutex<()>,
}

impl AccessImpl {
    pub(crate) fn new(project_root_locate: Arc<dyn ProjectRootLocate>) -> Self {
        Self {
            project_root_locate,
            lock: Mutex::new(()),
        }
    }
}

impl Access for AccessImpl {
    fn register(
        &self,
        is_cache: bool,
        env: &str,
        canister_name: &str,
        canister_id: Principal,
    ) -> Result<(), RegisterError> {
        // Lock ID Store
        let _g = self.lock.lock().expect("failed to acquire id store lock");

        let fpath = self.get_fpath_for_env(is_cache, env)?;

        // Load the file
        let mut mapping = load_mapping(&fpath).context(RegisterLoadStoreSnafu {
            env: env.to_owned(),
        })?;

        // Insert the new canister ID
        if let Some(existing_id) = mapping.insert(canister_name.to_owned(), canister_id) {
            return Err(RegisterError::AlreadyRegistered {
                env: env.to_owned(),
                canister_name: canister_name.to_owned(),
                id: existing_id,
            });
        }

        // Store JSON
        json::save(&fpath, &mapping).context(RegisterSaveStoreSnafu {
            env: env.to_owned(),
        })?;

        Ok(())
    }

    fn lookup(
        &self,
        is_cache: bool,
        env: &str,
        canister_name: &str,
    ) -> Result<Principal, LookupIdError> {
        let _g = self.lock.lock().expect("failed to acquire id store lock");
        let fpath = self.get_fpath_for_env(is_cache, env)?;
        load_mapping(&fpath)
            .context(LookupLoadStoreSnafu {
                env: env.to_owned(),
            })?
            .get(canister_name)
            .cloned()
            .ok_or_else(|| LookupIdError::IdNotFound {
                env: env.to_owned(),
                canister_name: canister_name.to_owned(),
            })
    }

    fn lookup_by_environment(&self, is_cache: bool, env: &str) -> Result<IdMapping, LookupIdError> {
        let _g = self.lock.lock().expect("failed to acquire id store lock");
        let fpath = self.get_fpath_for_env(is_cache, env)?;
        load_mapping(&fpath).context(LookupLoadStoreSnafu {
            env: env.to_owned(),
        })
    }
}

impl AccessImpl {
    /// Gets the ID mapping file path for a given environment.
    ///
    /// By default, the file is located at `{project_root}/.icp/{cache_or_data}/mappings/{env}.ids.json`.
    fn get_fpath_for_env(
        &self,
        is_cache: bool,
        env: &str,
    ) -> Result<PathBuf, ProjectRootLocateError> {
        let project_root = self.project_root_locate.locate()?;
        let base_path = if is_cache {
            project_root.join(CACHE_DIR)
        } else {
            project_root.join(DATA_DIR)
        };
        let fname = format!("{env}.ids.json");
        Ok(base_path.join(ICP_BASE).join("mappings").join(&fname))
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    /// In-memory mock implementation of `Access`.
    ///
    /// There are two separate stores for cache and data, to allow testing both paths.
    /// Each store keys on the environment name.
    /// The value is a mapping from canister names to their principals.
    pub(crate) struct MockInMemoryIdStore {
        cache: Mutex<BTreeMap<String, IdMapping>>,
        data: Mutex<BTreeMap<String, IdMapping>>,
    }

    impl MockInMemoryIdStore {
        /// Creates a new empty in-memory ID store.
        pub(crate) fn new() -> Self {
            Self {
                cache: Mutex::new(BTreeMap::new()),
                data: Mutex::new(BTreeMap::new()),
            }
        }
    }

    impl Default for MockInMemoryIdStore {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Access for MockInMemoryIdStore {
        fn register(
            &self,
            is_cache: bool,
            env: &str,
            canister_name: &str,
            canister_id: Principal,
        ) -> Result<(), RegisterError> {
            let mut store = if is_cache {
                self.cache.lock().unwrap()
            } else {
                self.data.lock().unwrap()
            };

            let mapping = store.entry(env.to_owned()).or_insert_with(BTreeMap::new);

            if let Some(existing_cid) = mapping.insert(canister_name.to_owned(), canister_id) {
                return Err(RegisterError::AlreadyRegistered {
                    env: env.to_owned(),
                    canister_name: canister_name.to_owned(),
                    id: existing_cid,
                });
            }

            Ok(())
        }

        fn lookup(
            &self,
            is_cache: bool,
            env: &str,
            canister_name: &str,
        ) -> Result<Principal, LookupIdError> {
            let store = if is_cache {
                self.cache.lock().unwrap()
            } else {
                self.data.lock().unwrap()
            };

            match store.get(env) {
                Some(mapping) => match mapping.get(canister_name) {
                    Some(cid) => Ok(*cid),
                    None => Err(LookupIdError::IdNotFound {
                        env: env.to_owned(),
                        canister_name: canister_name.to_owned(),
                    }),
                },
                None => Err(LookupIdError::EnvironmentNotFound {
                    name: env.to_owned(),
                }),
            }
        }

        fn lookup_by_environment(
            &self,
            is_cache: bool,
            env: &str,
        ) -> Result<IdMapping, LookupIdError> {
            let store = if is_cache {
                self.cache.lock().unwrap()
            } else {
                self.data.lock().unwrap()
            };
            match store.get(env) {
                Some(mapping) => Ok(mapping.clone()),
                None => Err(LookupIdError::EnvironmentNotFound {
                    name: env.to_owned(),
                }),
            }
        }
    }
}
