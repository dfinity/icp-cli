use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

use crate::canister::Settings;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct EnvironmentInner {
    pub name: String,
    pub network: Option<String>,
    pub canisters: Option<Vec<String>>,
    pub settings: Option<HashMap<String, Settings>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Default)]
pub enum CanisterSelection {
    /// No canisters are selected.
    None,

    /// A specific list of canisters is selected by name.
    /// An empty list is allowed, but `None` is preferred to indicate no selection.
    Named(Vec<String>),

    /// All canisters are selected.
    /// This is the default variant.
    #[default]
    Everything,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    // environment name
    pub name: String,

    // target network for canister deployment
    pub network: String,

    // canisters the environment should contain
    pub canisters: CanisterSelection,

    // canister settings overrides
    pub settings: Option<HashMap<String, Settings>>,
}

impl From<EnvironmentInner> for Environment {
    fn from(v: EnvironmentInner) -> Self {
        let EnvironmentInner {
            name,
            network,
            canisters,
            settings,
        } = v;

        // Network
        let network = network.unwrap_or("local".to_string());

        // Canisters
        let canisters = match canisters {
            // If the caller provided a list of canisters
            Some(cs) => match cs.is_empty() {
                // An empty list means explicitly "no canisters"
                true => CanisterSelection::None,

                // Non-empty list means targeting these specific canisters
                false => CanisterSelection::Named(cs),
            },

            // If no list was provided, assume all canisters are targeted
            None => CanisterSelection::Everything,
        };

        Self {
            name,
            network,
            canisters,

            // Keep as-is, setting overrides are optional
            settings,
        }
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: EnvironmentInner = Deserialize::deserialize(d)?;
        Ok(inner.into())
    }
}
