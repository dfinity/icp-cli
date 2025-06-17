use ic_agent::export::Principal;
use snafu::Snafu;
use std::sync::Mutex;

/// Association of a canister name and an ID
struct Association(String, Principal);

#[derive(Debug, Snafu)]
pub enum RegisterError {
    #[snafu(display("register error: {error}"))]
    Register { error: String },
}

pub trait Register {
    fn register(&self, name: &str, cid: &Principal) -> Result<(), RegisterError>;
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(display("could not find ID for canister '{name}'"))]
    LookupIdNotFound { name: String },
}

pub trait Lookup {
    fn lookup(&self, name: &str) -> Result<Principal, LookupError>;
}

pub struct CanisterStore(Mutex<Vec<Association>>);

impl CanisterStore {
    pub fn new() -> Self {
        Self(Mutex::new(vec![]))
    }
}

impl Register for CanisterStore {
    fn register(&self, name: &str, cid: &Principal) -> Result<(), RegisterError> {
        // TODO(or.ricon): decide if overwriting is allowed or not
        self.0
            .lock()
            .unwrap()
            .push(Association(name.to_owned(), cid.to_owned()));

        Ok(())
    }
}

impl Lookup for CanisterStore {
    fn lookup(&self, name: &str) -> Result<Principal, LookupError> {
        let vs = self.0.lock().unwrap();

        for Association(cname, cid) in vs.iter() {
            if name == cname {
                return Ok(cid.to_owned());
            }
        }

        Err(LookupError::LookupIdNotFound {
            name: name.to_owned(),
        })
    }
}
