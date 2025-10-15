use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum RecipeType {
    Unknown(String),
}

impl From<String> for RecipeType {
    fn from(value: String) -> Self {
        let other = value.as_str();
        Self::Unknown(other.to_owned())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Recipe {
    #[serde(rename = "type")]
    pub recipe_type: RecipeType,

    #[serde(default)]
    #[schemars(with = "HashMap<String, serde_json::Value>")]
    pub configuration: HashMap<String, serde_yaml::Value>,

    /// Optional sha256 checksum for the recipe template.
    /// If provided, the integrity of the recipe will be verified against this hash
    pub sha256: Option<String>,
}
