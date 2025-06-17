use camino::Utf8PathBuf;
use ic_agent::export::Principal;
use icp_fs::lockedjson;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

/// Association of a canister name and an ID
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Association(String, Principal);

#[derive(Debug, Snafu)]
pub enum RegisterError {
    #[snafu(display("failed to load canister store"))]
    RegisterLoadStore {
        source: lockedjson::LoadJsonWithLockError,
    },

    #[snafu(display("canister '{name}' is already registered with id '{id}'"))]
    RegisterAlreadyRegistered { name: String, id: Principal },

    #[snafu(display("failed to save canister store"))]
    RegisterSaveStore {
        source: lockedjson::SaveJsonWithLockError,
    },
}

pub trait Register {
    fn register(&self, name: &str, cid: &Principal) -> Result<(), RegisterError>;
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(display("failed to load canister store"))]
    LookupLoadStore {
        source: lockedjson::LoadJsonWithLockError,
    },

    #[snafu(display("could not find ID for canister '{name}'"))]
    LookupIdNotFound { name: String },
}

pub trait Lookup {
    fn lookup(&self, name: &str) -> Result<Principal, LookupError>;
}

pub struct CanisterStore(Utf8PathBuf);

impl CanisterStore {
    pub fn new(path: &Utf8PathBuf) -> Self {
        Self(path.clone())
    }
}

impl Register for CanisterStore {
    fn register(&self, name: &str, cid: &Principal) -> Result<(), RegisterError> {
        // Load JSON
        let mut cs: Vec<Association> = lockedjson::load_json_with_lock(&self.0)
            .context(RegisterLoadStoreSnafu)?
            .unwrap_or_default();

        // Check for existence
        for Association(cname, cid) in cs.iter() {
            if name == cname {
                return Err(RegisterError::RegisterAlreadyRegistered {
                    name: name.to_owned(),
                    id: *cid,
                });
            }
        }

        // Append
        cs.push(Association(name.to_owned(), cid.to_owned()));

        // Store JSON
        lockedjson::save_json_with_lock(&self.0, &cs).context(RegisterSaveStoreSnafu)?;

        Ok(())
    }
}

impl Lookup for CanisterStore {
    fn lookup(&self, name: &str) -> Result<Principal, LookupError> {
        // Load JSON
        let cs: Vec<Association> = lockedjson::load_json_with_lock(&self.0)
            .context(LookupLoadStoreSnafu)?
            .unwrap_or_default();

        // Search for association
        for Association(cname, cid) in cs {
            if name == cname {
                return Ok(cid.to_owned());
            }
        }

        // Not Found
        Err(LookupError::LookupIdNotFound {
            name: name.to_owned(),
        })
    }
}
