use std::{io::ErrorKind, sync::Mutex};

#[cfg(test)]
use std::collections::HashMap;

use crate::{fs::json, prelude::*};
use ic_agent::export::Principal;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

/// Trait for accessing and managing canister ID storage.
pub trait Access: Sync + Send {
    /// Register a canister ID for a given key.
    fn register(&self, key: &Key, cid: &Principal) -> Result<(), RegisterError>;

    /// Lookup a canister ID for a given key.
    fn lookup(&self, key: &Key) -> Result<Principal, LookupIdError>;

    /// Lookup all canister IDs for a given environment.
    fn lookup_by_environment(
        &self,
        environment: &str,
    ) -> Result<Vec<(String, Principal)>, LookupIdError>;
}

/// An association-key, used for associating an existing canister to an ID on a network
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    /// Network name
    pub network: String,

    /// Environment name
    pub environment: String,

    /// Canister name
    pub canister: String,
}

/// Association of a canister name and an ID
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Association(Key, Principal);

#[derive(Debug, Snafu)]
pub enum RegisterError {
    #[snafu(display("failed to load canister id store"))]
    RegisterLoadStore { source: json::Error },

    #[snafu(display(
        "canister '{}' in environment '{}', associated with network '{}' is already registered with id '{id}'",
        key.canister, key.environment, key.network,
    ))]
    AlreadyRegistered { key: Key, id: Principal },

    #[snafu(display("failed to save canister id store"))]
    RegisterSaveStore { source: json::Error },
}

#[derive(Debug, Snafu)]
pub enum LookupIdError {
    #[snafu(display("failed to load canister id store"))]
    LookupLoadStore { source: json::Error },

    #[snafu(display(
        "could not find ID for canister '{}' in environment '{}', associated with network '{}'",
        key.canister, key.environment, key.network
    ))]
    IdNotFound { key: Key },

    #[snafu(display("could not find canisters in environment '{}'", name))]
    EnvironmentNotFound { name: String },
}

pub(crate) struct IdStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl IdStore {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            lock: Mutex::new(()),
        }
    }
}

impl Access for IdStore {
    fn register(&self, key: &Key, cid: &Principal) -> Result<(), RegisterError> {
        // Lock ID Store
        let _g = self.lock.lock().expect("failed to acquire id store lock");

        // Load JSON
        let mut cs = json::load::<Vec<Association>>(&self.path)
            .or_else(|err| match err {
                // Default to empty
                json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(vec![]),

                // Other
                _ => Err(err),
            })
            .context(RegisterLoadStoreSnafu)?;

        // Check for existence
        for Association(k, cid) in cs.iter() {
            if k.canister == key.canister {
                return Err(RegisterError::AlreadyRegistered {
                    key: key.to_owned(),
                    id: *cid,
                });
            }
        }

        // Append
        cs.push(Association(key.to_owned(), cid.to_owned()));

        // Store JSON
        json::save(&self.path, &cs).context(RegisterSaveStoreSnafu)?;

        Ok(())
    }

    fn lookup(&self, key: &Key) -> Result<Principal, LookupIdError> {
        // Lock ID Store
        let _g = self.lock.lock().expect("failed to acquire id store lock");

        // Load JSON
        let cs = json::load::<Vec<Association>>(&self.path)
            .or_else(|err| match err {
                // Default to empty
                json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(vec![]),

                // Other
                _ => Err(err),
            })
            .context(LookupLoadStoreSnafu)?;

        // Search for association
        for Association(k, cid) in cs {
            if k.canister == key.canister {
                return Ok(cid.to_owned());
            }
        }

        // Not Found
        Err(LookupIdError::IdNotFound {
            key: key.to_owned(),
        })
    }

    fn lookup_by_environment(
        &self,
        environment: &str,
    ) -> Result<Vec<(String, Principal)>, LookupIdError> {
        // Lock ID Store
        let _g = self.lock.lock().expect("failed to acquire id store lock");

        // Load JSON
        let cs = json::load::<Vec<Association>>(&self.path)
            .or_else(|err| match err {
                // Default to empty
                json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(vec![]),

                // Other
                _ => Err(err),
            })
            .context(LookupLoadStoreSnafu)?;

        let filtered_associations: Vec<(String, Principal)> = cs
            .into_iter()
            .filter(|Association(k, _)| k.environment == *environment)
            .map(|Association(k, cid)| (k.canister, cid))
            .collect();

        if filtered_associations.is_empty() {
            return Err(LookupIdError::EnvironmentNotFound {
                name: environment.to_owned(),
            });
        }

        Ok(filtered_associations)
    }
}

#[cfg(test)]
/// In-memory mock implementation of `Access`.
pub(crate) struct MockInMemoryIdStore {
    store: Mutex<HashMap<Key, Principal>>,
}

#[cfg(test)]
impl MockInMemoryIdStore {
    /// Creates a new empty in-memory ID store.
    pub(crate) fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
impl Default for MockInMemoryIdStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl Access for MockInMemoryIdStore {
    fn register(&self, key: &Key, cid: &Principal) -> Result<(), RegisterError> {
        let mut store = self.store.lock().unwrap();

        // Check if canister already registered
        if let Some(existing_cid) = store.get(key) {
            return Err(RegisterError::AlreadyRegistered {
                key: key.to_owned(),
                id: *existing_cid,
            });
        }

        // Store the association
        store.insert(key.clone(), *cid);

        Ok(())
    }

    fn lookup(&self, key: &Key) -> Result<Principal, LookupIdError> {
        let store = self.store.lock().unwrap();

        match store.get(key) {
            Some(cid) => Ok(*cid),
            None => Err(LookupIdError::IdNotFound {
                key: key.to_owned(),
            }),
        }
    }

    fn lookup_by_environment(
        &self,
        environment: &str,
    ) -> Result<Vec<(String, Principal)>, LookupIdError> {
        let store = self.store.lock().unwrap();

        let filtered: Vec<(String, Principal)> = store
            .iter()
            .filter(|(k, _)| k.environment == environment)
            .map(|(k, cid)| (k.canister.clone(), *cid))
            .collect();

        if filtered.is_empty() {
            return Err(LookupIdError::EnvironmentNotFound {
                name: environment.to_owned(),
            });
        }

        Ok(filtered)
    }
}
