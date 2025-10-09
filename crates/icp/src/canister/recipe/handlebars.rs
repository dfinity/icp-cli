use std::{collections::HashMap, str::FromStr, string::FromUtf8Error};

use crate::{
    canister::{
        build,
        recipe::{Resolve, ResolveError},
        sync,
    },
    fs::read,
    manifest::{
        canister::Instructions,
        recipe::{Recipe, RecipeType},
    },
    prelude::*,
};
use async_trait::async_trait;
use handlebars::{Context, Helper, HelperDef, HelperResult, Output};
use reqwest::{Method, Request, Url};
use url::ParseError;

pub struct Handlebars {
    /// Built-in recipe templates
    pub recipes: HashMap<String, String>,

    /// Http client for fetching remote recipe templates
    pub http_client: reqwest::Client,
}

pub enum TemplateSource {
    BuiltIn(String),
    LocalPath(PathBuf),
    RemoteUrl(String),

    /// Template originating in a remote registry, e.g `@dfinity/rust@v1.0.2`
    Registry(String, String, String),
}

#[derive(Debug, thiserror::Error)]
pub enum HandlebarsError {
    #[error("no recipe found for recipe type '{recipe}'")]
    Unknown { recipe: String },

    #[error("failed to read local recipe template file")]
    ReadFile { source: crate::fs::Error },

    #[error("failed to decode UTF-8 string")]
    DecodeUtf8 { source: FromUtf8Error },

    #[error("failed to parse user-provided url")]
    UrlParse { source: ParseError },

    #[error("failed to execute http request")]
    HttpRequest { source: reqwest::Error },

    #[error("request returned non-ok status-code")]
    HttpStatus { status: u16 },

    #[error("the partrial template for partial '{partial}' appears to be invalid")]
    PartialInvalid {
        source: handlebars::TemplateError,
        partial: String,
        template: String,
    },

    #[error("the recipe template for recipe type '{recipe}' failed to be rendered")]
    Render {
        source: handlebars::RenderError,
        recipe: String,
        template: String,
    },
}

#[async_trait]
impl Resolve for Handlebars {
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError> {
        // Sanity check recipe type
        let recipe_type = match &recipe.recipe_type {
            RecipeType::Unknown(typ) => typ.to_owned(),
            _ => panic!("expected unknown recipe"),
        };

        // Infer source for recipe template (local, remote, built-in, etc)
        let tmpl = (|recipe_type: String| {
            if recipe_type.starts_with("file://") {
                let path = recipe_type
                    .strip_prefix("file://")
                    .map(Path::new)
                    .expect("prefix missing")
                    .into();

                return TemplateSource::LocalPath(path);
            }

            if recipe_type.starts_with("http://") || recipe_type.starts_with("https://") {
                return TemplateSource::RemoteUrl(recipe_type);
            }

            if recipe_type.starts_with("@") {
                let recipe_type = recipe_type.strip_prefix("@").expect("prefix missing");

                // Check for version delimiter
                let (v, version) = if recipe_type.contains("@") {
                    // Version is specified
                    recipe_type.rsplit_once("@").expect("delimiter missing")
                } else {
                    // Assume latest
                    (recipe_type, "latest")
                };

                let (registry, recipe) = v.split_once("/").expect("delimiter missing");

                return TemplateSource::Registry(
                    registry.to_owned(),
                    recipe.to_owned(),
                    version.to_owned(),
                );
            }

            TemplateSource::BuiltIn(recipe_type)
        })(recipe_type.clone());

        // TMP(or.ricon): Temporarily hardcode a dfinity registry
        let tmpl = match tmpl {
            TemplateSource::Registry(registry, recipe, version) => {
                if registry != "dfinity" {
                    panic!("only the dfinity registry is currently supported");
                }

                TemplateSource::RemoteUrl(format!(
                    "https://github.com/rikonor/icp-recipes/releases/download/{recipe}-{version}/recipe.hbs"
                ))
            }
            _ => tmpl,
        };

        // Retrieve the template for the recipe from its respective source
        let tmpl = match tmpl {
            // Search for built-in recipe template
            TemplateSource::BuiltIn(typ) => self
                .recipes
                .iter()
                .find_map(|(name, tmpl)| {
                    if name == &typ {
                        Some(tmpl.to_owned())
                    } else {
                        None
                    }
                })
                .ok_or(ResolveError::Handlebars {
                    source: HandlebarsError::Unknown {
                        recipe: typ.to_owned(),
                    },
                })?,

            // TMP(or.ricon): Support multiple registries
            TemplateSource::Registry(_, _, _) => panic!(
                "registry source should have been converted to a dfinity-specific remote url"
            ),

            // Attempt to load template from local file-system
            TemplateSource::LocalPath(path) => {
                let bs = read(&path).map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::ReadFile { source: err },
                })?;

                String::from_utf8(bs).map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::DecodeUtf8 { source: err },
                })?
            }

            // Attempt to fetch template from remote resource url
            TemplateSource::RemoteUrl(u) => {
                let u = Url::from_str(&u).map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::UrlParse { source: err },
                })?;

                let resp = self
                    .http_client
                    .execute(Request::new(Method::GET, u))
                    .await
                    .map_err(|err| ResolveError::Handlebars {
                        source: HandlebarsError::HttpRequest { source: err },
                    })?;

                if !resp.status().is_success() {
                    return Err(ResolveError::Handlebars {
                        source: HandlebarsError::HttpStatus {
                            status: resp.status().as_u16(),
                        },
                    });
                }

                resp.text().await.map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::HttpRequest { source: err },
                })?
            }
        };

        // Load the template via handlebars
        let mut reg = handlebars::Handlebars::new();

        // Register helpers
        reg.register_helper("replace", Box::new(ReplaceHelper));

        // Register partials for reusable template components
        // These partials provide common functionality like WASM optimization and metadata injection
        for (name, partial) in PARTIALS {
            reg.register_partial(name, partial)
                .map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::PartialInvalid {
                        source: err,
                        partial: name.to_owned(),
                        template: partial.to_owned(),
                    },
                })?;
        }

        // Reject unset template variables
        reg.set_strict_mode(true);

        // Render the template to YAML
        let out = reg
            .render_template(&tmpl, &recipe.configuration)
            .map_err(|err| ResolveError::Handlebars {
                source: HandlebarsError::Render {
                    source: err,
                    recipe: recipe_type.to_owned(),
                    template: tmpl.to_owned(),
                },
            })?;

        // Read the rendered YAML canister manifest
        let insts = serde_yaml::from_str::<Instructions>(&out).unwrap();

        let (build, sync) = match insts {
            // Supported
            Instructions::BuildSync { build, sync } => (build, sync),

            // Unsupported
            Instructions::Recipe { .. } => {
                panic!("recipe within a recipe is not supported")
            }
        };

        Ok((build, sync))
    }
}

/// Handlebars helper for string replacement operations
/// Usage: {{ replace "from" "to" value }}
#[derive(Clone, Copy)]
struct ReplaceHelper;

impl HelperDef for ReplaceHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &'reg handlebars::Handlebars<'reg>,
        _: &Context,
        _: &mut handlebars::RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let (from, to) = (
            h.param(0).unwrap().render(), // from
            h.param(1).unwrap().render(), // to
        );

        let v = h.param(2).unwrap().render();
        out.write(&v.replace(&from, &to))?;

        Ok(())
    }
}

/// Handlebars partial for shrinking WASM modules using ic-wasm
/// Reduces module size by removing unnecessary sections while preserving name sections
pub const WASM_SHRINK_PARTIAL: &str = r#"
- type: script
  commands:
    - sh -c 'command -v ic-wasm >/dev/null 2>&1 || { echo >&2 "ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n"; exit 1; }'
    - sh -c 'ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" shrink --keep-name-section'
"#;

/// Handlebars partial for compressing WASM modules using gzip
/// Reduces deployment size and costs by applying gzip compression
pub const WASM_COMPRESS_PARTIAL: &str = r#"
- type: script
  commands:
    - sh -c 'command -v gzip >/dev/null 2>&1 || { echo >&2 "gzip not found. Please install gzip to compress build output. \n"; exit 1; }'
    - sh -c 'gzip --no-name "$ICP_WASM_OUTPUT_PATH"'
    - sh -c 'mv "${ICP_WASM_OUTPUT_PATH}.gz" "$ICP_WASM_OUTPUT_PATH"'
"#;

/// Handlebars partial that combines shrinking and compression optimizations
/// Provides a convenient way to apply both wasm-shrink and wasm-compress operations
pub const WASM_OPTIMIZE_PARTIAL: &str = r#"
{{> wasm-shrink }}
{{> wasm-compress }}
"#;

/// Handlebars partial for injecting custom metadata into WASM modules
/// Expects 'name' and 'value' variables to be set in the template context
pub const WASM_INJECT_METADATA_PARTIAL: &str = r#"
- type: script
  commands:
    - sh -c 'command -v ic-wasm >/dev/null 2>&1 || { echo >&2 "ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n"; exit 1; }'
    - sh -c 'ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "{{ name }}" -d "{{ value }}" --keep-name-section'
"#;

/// Collection of reusable Handlebars partials for WASM processing
/// These partials can be included in templates using {{> partial-name}} syntax
pub const PARTIALS: [(&str, &str); 4] = [
    ("wasm-shrink", WASM_SHRINK_PARTIAL),
    ("wasm-compress", WASM_COMPRESS_PARTIAL),
    ("wasm-optimize", WASM_OPTIMIZE_PARTIAL),
    ("wasm-inject-metadata", WASM_INJECT_METADATA_PARTIAL),
];

/// Template for pre-built canister recipes
/// Supports optional shrink, compress, and metadata configuration
pub const PREBUILT_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: pre-built
      path: {{ path }}
      sha256: {{ sha256 }}

    {{> wasm-inject-metadata name="template:type" value="pre-built" }}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata name=name value=value }}
    {{/each}}
    {{/if}}

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}
"#;

/// Template for assets canister recipes
/// Downloads the official assets canister WASM and configures asset synchronization
pub const ASSETS_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: pre-built
      url: https://github.com/dfinity/sdk/raw/refs/tags/{{ version }}/src/distributed/assetstorage.wasm.gz

    {{> wasm-inject-metadata name="template:type" value="assets" }}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata name=name value=value }}
    {{/each}}
    {{/if}}

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}

sync:
  steps:
    - type: assets
      dir: {{ dir }}
"#;

/// Template for Motoko canister recipes
/// Compiles Motoko source code using the moc compiler
pub const MOTOKO_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: script
      commands:
        - sh -c 'command -v moc >/dev/null 2>&1 || { echo >&2 "moc not found. To install moc, see https://internetcomputer.org/docs/building-apps/getting-started/install \n"; exit 1; }'
        - sh -c 'moc {{ entry }}'
        - sh -c 'mv main.wasm "$ICP_WASM_OUTPUT_PATH"'

    - type: script
      commands:
        - sh -c 'ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "moc:version" -d "$(moc --version)" --keep-name-section'

    {{> wasm-inject-metadata name="template:type" value="motoko" }}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata name=name value=value }}
    {{/each}}
    {{/if}}

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}
"#;

/// Template for Rust canister recipes
/// Builds Rust canisters using Cargo with WASM target
pub const RUST_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: script
      commands:
        - cargo build --package {{ package }} --target wasm32-unknown-unknown --release
        - sh -c 'mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" package }}.wasm "$ICP_WASM_OUTPUT_PATH"'

    - type: script
      commands:
        - sh -c 'ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "cargo:version" -d "$(cargo --version)" --keep-name-section'

    {{> wasm-inject-metadata name="template:type" value="rust" }}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata name=name value=value }}
    {{/each}}
    {{/if}}

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}
"#;

/// Collection of available Handlebars templates for different canister types
/// Maps recipe type names to their corresponding template definitions
pub const TEMPLATES: [(&str, &str); 4] = [
    ("prebuilt", PREBUILT_CANISTER_TEMPLATE),
    ("handlebars-assets", ASSETS_CANISTER_TEMPLATE),
    ("handlebars-motoko", MOTOKO_CANISTER_TEMPLATE),
    ("handlebars-rust", RUST_CANISTER_TEMPLATE),
];
