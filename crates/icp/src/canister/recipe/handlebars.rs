use indoc::formatdoc;
use serde::Deserialize;
use std::{str::FromStr, string::FromUtf8Error};
use tracing::debug;

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
use sha2::{Digest, Sha256};
use url::ParseError;

pub struct Handlebars {
    /// Http client for fetching remote recipe templates
    pub http_client: reqwest::Client,
}

pub enum TemplateSource {
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

    #[error("sha256 checksum mismatch for recipe template: expected {expected}, actual {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

#[async_trait]
impl Resolve for Handlebars {
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError> {
        // Find the template
        let tmpl = match &recipe.recipe_type {
            RecipeType::File(path) => TemplateSource::LocalPath(Path::new(&path).into()),
            RecipeType::Url(url) => TemplateSource::RemoteUrl(url.to_owned()),
            RecipeType::Registry {
                name,
                recipe,
                version,
            } => TemplateSource::Registry(name.to_owned(), recipe.to_owned(), version.to_owned()),
        };

        // TMP(or.ricon): Temporarily hardcode a dfinity registry
        let tmpl = match tmpl {
            TemplateSource::Registry(registry, recipe, version) => {
                if registry != "dfinity" {
                    panic!("only the dfinity registry is currently supported");
                }

                TemplateSource::RemoteUrl(format!(
                    "https://github.com/dfinity/icp-cli-recipes/releases/download/{recipe}-{version}/recipe.hbs"
                ))
            }
            _ => tmpl,
        };

        // Retrieve the template for the recipe from its respective source
        let tmpl = match tmpl {
            // TMP(or.ricon): Support multiple registries
            TemplateSource::Registry(_, _, _) => panic!(
                "registry source should have been converted to a dfinity-specific remote url"
            ),

            // Attempt to load template from local file-system
            TemplateSource::LocalPath(path) => {
                let bytes = read(&path).map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::ReadFile { source: err },
                })?;

                // Verify the checksum if it's provided
                if let Some(expected) = &recipe.sha256 {
                    verify_checksum(&bytes, expected)
                        .map_err(|source| ResolveError::Handlebars { source: *source })?;
                }

                parse_bytes_to_string(bytes)
                    .map_err(|source| ResolveError::Handlebars { source: *source })?
            }

            // Attempt to fetch template from remote resource url
            TemplateSource::RemoteUrl(u) => {
                let u = Url::from_str(&u).map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::UrlParse { source: err },
                })?;

                debug!("Requesting template from: {u}");

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

                let bytes = resp.bytes().await.map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::HttpRequest { source: err },
                })?;

                // Verify the checksum if it's provided
                if let Some(expected) = &recipe.sha256 {
                    verify_checksum(&bytes, expected)
                        .map_err(|source| ResolveError::Handlebars { source: *source })?;
                }

                parse_bytes_to_string(bytes.into())
                    .map_err(|source| ResolveError::Handlebars { source: *source })?
            }
        };

        // Load the template via handlebars
        let mut reg = handlebars::Handlebars::new();

        // Register helpers
        reg.register_helper("replace", Box::new(ReplaceHelper));

        // Reject unset template variables
        reg.set_strict_mode(true);

        debug!(
            "{}",
            formatdoc! {r#"
            Loaded template:
            ------
            {tmpl}
            ------
        "#}
        );

        // Render the template to YAML
        let out = reg
            .render_template(&tmpl, &recipe.configuration)
            .map_err(|err| ResolveError::Handlebars {
                source: HandlebarsError::Render {
                    source: err,
                    recipe: recipe.recipe_type.clone().into(),
                    template: tmpl.to_owned(),
                },
            })?;

        // Read the rendered YAML canister manifest
        // Recipes can only render buid/sync
        #[derive(Deserialize)]
        struct BuildSyncHelper {
            build: build::Steps,
            #[serde(default)]
            sync: sync::Steps,
        }

        let insts = serde_yaml::from_str::<BuildSyncHelper>(&out);
        let insts = match insts {
            Ok(helper) => Instructions::BuildSync {
                build: helper.build,
                sync: helper.sync,
            },
            Err(e) => panic!(
                "{}",
                formatdoc! {r#"
                Unable to render recipe {} template into valid yaml: {e}

                Rendered content:
                ------
                {out}
                ------
            "#, recipe.recipe_type}
            ),
        };

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

/// Helper function to verify sha256 checksum of recipe template bytes
fn verify_checksum(bytes: &[u8], expected: &str) -> Result<(), Box<HandlebarsError>> {
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize()
    });

    if actual != expected {
        return Err(Box::new(HandlebarsError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        }));
    }

    Ok(())
}

/// Helper function to parse bytes into a UTF-8 string
fn parse_bytes_to_string(bytes: Vec<u8>) -> Result<String, Box<HandlebarsError>> {
    String::from_utf8(bytes).map_err(|err| Box::new(HandlebarsError::DecodeUtf8 { source: err }))
}
