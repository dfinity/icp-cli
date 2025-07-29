use camino::Utf8PathBuf;
use ic_agent::export::Principal;
use icp_fs::lockedjson;
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
    RegisterLoadStore {
        source: lockedjson::LoadJsonWithLockError,
    },

    #[snafu(display(
        "canister '{}' in environment '{}', associated with network '{}' is already registered with id '{id}'",
        key.canister, key.environment, key.network,
    ))]
    AlreadyRegistered { key: Key, id: Principal },

    #[snafu(display("failed to save canister id store"))]
    RegisterSaveStore {
        source: lockedjson::SaveJsonWithLockError,
    },
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(display("failed to load canister id store"))]
    LookupLoadStore {
        source: lockedjson::LoadJsonWithLockError,
    },

    #[snafu(display(
        "could not find ID for canister '{}' in environment '{}', associated with network '{}'",
        key.canister, key.environment, key.network
    ))]
    IdNotFound { key: Key },
}

pub struct IdStore(Utf8PathBuf);

impl IdStore {
    pub fn new(path: &Utf8PathBuf) -> Self {
        Self(path.clone())
    }
}

impl IdStore {
    pub fn register(&self, key: &Key, cid: &Principal) -> Result<(), RegisterError> {
        // Load JSON
        let mut cs: Vec<Association> = lockedjson::load_json_with_lock(&self.0)
            .context(RegisterLoadStoreSnafu)?
            .unwrap_or_default();

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
        lockedjson::save_json_with_lock(&self.0, &cs).context(RegisterSaveStoreSnafu)?;

        Ok(())
    }

    pub fn lookup(&self, key: &Key) -> Result<Principal, LookupError> {
        // Load JSON
        let cs: Vec<Association> = lockedjson::load_json_with_lock(&self.0)
            .context(LookupLoadStoreSnafu)?
            .unwrap_or_default();

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
}
