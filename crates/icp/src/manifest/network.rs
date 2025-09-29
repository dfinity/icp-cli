use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Managed,
    Connected,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Network {
    pub name: String,
    pub mode: Mode,
}
