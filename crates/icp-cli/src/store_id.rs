use std::io::ErrorKind;

use ic_agent::export::Principal;
use icp::{fs::json, prelude::*};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

/// An association-key, used for associating an existing canister to an ID on a network
#[derive(Clone, Debug, Serialize, Deserialize)]
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
pub enum LookupError {
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

pub struct IdStore {
    path: PathBuf,
}

impl IdStore {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }
}

impl IdStore {
    pub fn register(&self, key: &Key, cid: &Principal) -> Result<(), RegisterError> {
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

    pub fn lookup(&self, key: &Key) -> Result<Principal, LookupError> {
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

    pub fn lookup_by_environment(
        &self,
        environment: &str,
    ) -> Result<Vec<(String, Principal)>, LookupError> {
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
