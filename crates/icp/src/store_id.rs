use std::fs::create_dir_all;
use std::{io::ErrorKind, sync::Mutex};

use std::collections::BTreeMap;

use crate::{fs::json, prelude::*};
use ic_agent::export::Principal;
use snafu::{ResultExt, Snafu};

/// Mapping of canister names to their Principals within an environment.
type IdMapping = BTreeMap<String, Principal>;

/// Trait for accessing and managing canister ID storage.
pub trait Access: Sync + Send {
    /// Register a mapping of (canister name, canister ID) for a given environment.
    fn register(
        &self,
        env: &str,
        canister_name: &str,
        canister_id: Principal,
    ) -> Result<(), RegisterError>;

    /// Lookup canister ID of a canister name in an environment.
    fn lookup(&self, env: &str, canister_name: &str) -> Result<Principal, LookupIdError>;

    /// Lookup all canister IDs for a given environment.
    fn lookup_by_environment(&self, env: &str) -> Result<IdMapping, LookupIdError>;
}

#[derive(Debug, Snafu)]
pub enum RegisterError {
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
pub(crate) struct IdStore {
    // Path to the directory which contains the canister mapping files for each environment.
    path: PathBuf,
    lock: Mutex<()>,
}

impl IdStore {
    pub(crate) fn new(path: &Path) -> Self {
        // TODO: DirectoryStructureLock::open_or_create will ensure the directory is created.
        create_dir_all(path).expect("failed to create id store directory");
        Self {
            path: path.to_owned(),
            lock: Mutex::new(()),
        }
    }
}

impl Access for IdStore {
    fn register(
        &self,
        env: &str,
        canister_name: &str,
        canister_id: Principal,
    ) -> Result<(), RegisterError> {
        // Lock ID Store
        let _g = self.lock.lock().expect("failed to acquire id store lock");

        let fpath = self.get_fpath_for_env(env);

        // Load the file
        let mut mapping = self.load_mapping(env).context(RegisterLoadStoreSnafu {
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

    fn lookup(&self, env: &str, canister_name: &str) -> Result<Principal, LookupIdError> {
        let _g = self.lock.lock().expect("failed to acquire id store lock");
        self.load_mapping(env)
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

    fn lookup_by_environment(&self, env: &str) -> Result<IdMapping, LookupIdError> {
        let _g = self.lock.lock().expect("failed to acquire id store lock");

        self.load_mapping(env).context(LookupLoadStoreSnafu {
            env: env.to_owned(),
        })
    }
}

impl IdStore {
    /// Gets the ID mapping file path for a given environment.
    ///
    /// The filename is constructed as `{env}.ids.json`.
    fn get_fpath_for_env(&self, env: &str) -> PathBuf {
        let fname = format!("{env}.ids.json");
        self.path.join(&fname)
    }

    /// Loads the ID mapping for a given environment.
    ///
    /// If the file does not exist, returns an empty mapping.
    fn load_mapping(&self, env: &str) -> Result<IdMapping, json::Error> {
        let fpath = self.get_fpath_for_env(env);
        json::load(&fpath).or_else(|err| match err {
            // Default to empty
            json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(BTreeMap::new()),

            // Other
            _ => Err(err),
        })
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    /// In-memory mock implementation of `Access`.
    pub(crate) struct MockInMemoryIdStore {
        /// The store keys on the environment name.
        /// The value is a mapping from canister names to their principals.
        store: Mutex<BTreeMap<String, IdMapping>>,
    }

    impl MockInMemoryIdStore {
        /// Creates a new empty in-memory ID store.
        pub(crate) fn new() -> Self {
            Self {
                store: Mutex::new(BTreeMap::new()),
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
            env: &str,
            canister_name: &str,
            canister_id: Principal,
        ) -> Result<(), RegisterError> {
            let mut store = self.store.lock().unwrap();

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

        fn lookup(&self, env: &str, canister_name: &str) -> Result<Principal, LookupIdError> {
            let store = self.store.lock().unwrap();

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

        fn lookup_by_environment(&self, env: &str) -> Result<IdMapping, LookupIdError> {
            let store = self.store.lock().unwrap();

            match store.get(env) {
                Some(mapping) => Ok(mapping.clone()),
                None => Err(LookupIdError::EnvironmentNotFound {
                    name: env.to_owned(),
                }),
            }
        }
    }
}
