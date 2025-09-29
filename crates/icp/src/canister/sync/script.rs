use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Adapter;

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}
