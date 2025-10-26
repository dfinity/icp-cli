use std::{collections::HashMap, fmt::Display};

use schemars::JsonSchema;
use serde::Deserialize;

/// Represents the accepted values for a recipe type in
/// the canister manifest
#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum RecipeType {
    /// path to a locally defined recipe
    File(String),

    /// url to a remote recipe
    Url(String),

    /// A recipe hosted in a known registry
    /// in yaml, the format is "@<registry>/<recipe>@<version>"
    Registry {
        /// the name of registry
        name: String,

        /// the name of the recipe
        recipe: String,

        /// the version of the recipe, deserializes to `latest` when not provided
        version: String,
    },
}

impl Display for RecipeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: String = self.clone().into();
        write!(f, "{s}")
    }
}

impl<'de> Deserialize<'de> for RecipeType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = String::deserialize(deserializer)?;

        if v.starts_with("file://") {
            let path = v.strip_prefix("file://").expect("prefix missing").into();

            return Ok(Self::File(path));
        }

        if v.starts_with("http://") || v.starts_with("https://") {
            return Ok(Self::Url(v.to_owned()));
        }

        if v.starts_with("@") {
            let recipe_type = v.strip_prefix("@").expect("prefix missing");

            // Check for version delimiter
            let (v, version) = if recipe_type.contains("@") {
                // Version is specified
                recipe_type.rsplit_once("@").expect("delimiter missing")
            } else {
                // Assume latest
                (recipe_type, "latest")
            };

            let (registry, recipe) = v.split_once("/").expect("delimiter missing");

            return Ok(Self::Registry {
                name: registry.to_owned(),
                recipe: recipe.to_owned(),
                version: version.to_owned(),
            });
        }

        Err(serde::de::Error::custom(format!(
            "Invalid recipe type: {v}"
        )))
    }
}

impl From<RecipeType> for String {
    fn from(value: RecipeType) -> Self {
        match value {
            RecipeType::File(path) => format!("file://{path}"),
            RecipeType::Url(url) => url,
            RecipeType::Registry {
                name,
                recipe,
                version,
            } => format!("@{name}/{recipe}@{version}"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Recipe {
    /// An identifier for a recipe, it can have one of the following formats:
    ///
    /// `file://<path_to_recipe>` - point to a local recipe template
    ///
    /// `http://<url_to_recipe>` - point to a remote recipe template
    ///
    /// `@<registry>/<recipe_name>[@<version>]` - Point to a recipe in a known registry.
    ///
    /// For now the only registry is the `dfinity` regitry at https://github.com/dfinity/icp-cli-recipes
    /// `version` is optional and defaults to `latest`
    ///
    /// It is always recommended to pin the version and provide a hash for it in the `sha256` field
    #[serde(rename = "type")]
    #[schemars(with = "String")]
    pub recipe_type: RecipeType,

    #[serde(default)]
    #[schemars(with = "Option<HashMap<String, serde_json::Value>>")]
    pub configuration: HashMap<String, serde_yaml::Value>,

    /// Optional sha256 checksum for the recipe template.
    /// If provided, the integrity of the recipe will be verified against this hash
    pub sha256: Option<String>,
}
