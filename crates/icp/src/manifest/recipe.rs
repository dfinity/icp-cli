use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum RecipeType {
    Assets,
    Motoko,
    Rust,
    Unknown(String),
}

impl From<String> for RecipeType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "assets" => Self::Assets,
            "motoko" => Self::Rust,
            "rust" => Self::Rust,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Recipe {
    #[serde(rename = "type")]
    pub recipe_type: RecipeType,

    #[serde(default)]
    #[schemars(with = "HashMap<String, serde_json::Value>")]
    pub configuration: HashMap<String, serde_yaml::Value>,

    /// Optional sha256 checksum for the recipe template,
    /// useful for verifying the integrity of remote recipe templates
    pub sha256: Option<String>,
}
