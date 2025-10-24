use std::{io::ErrorKind, sync::Mutex};

use ic_agent::export::Principal;
use icp::{fs::json, prelude::*};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

/// An association-key, used for associating an existing canister to an ID on a network
#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub(crate) struct Key {
    /// Network name
    pub(crate) network: String,

    /// Environment name
    pub(crate) environment: String,

    /// Canister name
    pub(crate) canister: String,
}

/// Association of a canister name and an ID
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Association(Key, Principal);

#[derive(Debug, Snafu)]
pub(crate) enum RegisterError {
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
pub(crate) enum LookupError {
    #[snafu(display("failed to load canister id store"))]
    LookupLoadStore { source: json::Error },

    #[snafu(display(
        "could not find ID for canister '{}' in environment '{}', associated with network '{}'",
        canister,
        environment,
        network
    ))]
    NotFound {
        canister: String,
        environment: String,
        network: String,
    },

    #[snafu(display(
        "could not find ID for canister '{}' in environment '{}', associated with network '{}'",
        key.canister, key.environment, key.network
    ))]
    IdNotFound { key: Key },

    #[snafu(display("could not find canisters in environment '{}'", name))]
    EnvironmentNotFound { name: String },
}

pub(crate) trait IdStore: Send + Sync {
    fn register(&self, key: &Key, cid: &Principal) -> Result<(), RegisterError>;
    fn lookup(&self, key: &Key) -> Result<Principal, LookupError>;
    fn lookup_by_environment(
        &self,
        environment: &str,
    ) -> Result<Vec<(String, Principal)>, LookupError>;
}

pub(crate) struct FileIdStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileIdStore {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            lock: Mutex::new(()),
        }
    }
}

impl IdStore for FileIdStore {
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

    fn lookup(&self, key: &Key) -> Result<Principal, LookupError> {
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
        Err(LookupError::IdNotFound {
            key: key.to_owned(),
        })
    }

    fn lookup_by_environment(
        &self,
        environment: &str,
    ) -> Result<Vec<(String, Principal)>, LookupError> {
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
            return Err(LookupError::EnvironmentNotFound {
                name: environment.to_owned(),
            });
        }

        Ok(filtered_associations)
    }
}

// ============================================================================
// Test utilities
// ============================================================================

#[cfg(test)]
pub(crate) mod test {
    use std::collections::HashMap;

    use super::*;

    /// Mock ID store for testing.
    ///
    /// Stores canister IDs in memory instead of on disk.
    pub(crate) struct MockIdStore {
        ids: HashMap<Key, Principal>,
    }

    impl MockIdStore {
        pub(crate) fn new() -> Self {
            Self {
                ids: HashMap::new(),
            }
        }

        pub(crate) fn with_id(mut self, key: Key, id: Principal) -> Self {
            self.ids.insert(key, id);
            self
        }
    }

    impl IdStore for MockIdStore {
        fn lookup(&self, key: &Key) -> Result<Principal, LookupError> {
            self.ids
                .get(key)
                .cloned()
                .ok_or_else(|| LookupError::NotFound {
                    network: key.network.clone(),
                    environment: key.environment.clone(),
                    canister: key.canister.clone(),
                })
        }

        fn register(&self, _key: &Key, _id: &Principal) -> Result<(), RegisterError> {
            // Mock doesn't support registration after creation
            Ok(())
        }

        fn lookup_by_environment(
            &self,
            environment: &str,
        ) -> Result<Vec<(String, Principal)>, LookupError> {
            let filtered: Vec<_> = self
                .ids
                .iter()
                .filter(|(k, _)| k.environment == environment)
                .map(|(k, v)| (k.canister.clone(), *v))
                .collect();

            if filtered.is_empty() {
                Err(LookupError::EnvironmentNotFound {
                    name: environment.to_string(),
                })
            } else {
                Ok(filtered)
            }
        }
    }
}
