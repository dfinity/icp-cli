use std::{str::FromStr, string::FromUtf8Error};

use async_trait::async_trait;
use handlebars::{Context, Helper, HelperDef, HelperResult, Output};
use indoc::formatdoc;
use reqwest::{Method, Request, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tracing::debug;
use url::ParseError;

use crate::{
    fs::read,
    manifest::{
        canister::{BuildSteps, SyncSteps},
        recipe::{Recipe, RecipeType},
    },
    package::{PackageCache, cache_recipe, read_cached_recipe},
    prelude::*,
};

use super::{Resolve, ResolveError};

pub struct Handlebars {
    /// Http client for fetching remote recipe templates
    pub http_client: reqwest::Client,
    /// Package cache for caching downloaded recipe templates
    pub pkg_cache: PackageCache,
}

pub enum TemplateSource {
    LocalPath(PathBuf),
    RemoteUrl(String),

    /// Template originating in a remote registry, e.g `@dfinity/rust@v1.0.2`
    Registry(String, String, String),
}

#[derive(Debug, Snafu)]
pub enum HandlebarsError {
    #[snafu(display("failed to read local recipe template file"))]
    ReadFile { source: crate::fs::IoError },

    #[snafu(display("failed to decode UTF-8 string"))]
    DecodeUtf8 { source: FromUtf8Error },

    #[snafu(display("failed to parse user-provided url"))]
    UrlParse { source: ParseError },

    #[snafu(display("failed to execute http request"))]
    HttpRequest { source: reqwest::Error },

    #[snafu(display("request to '{url}' returned '{status}' status-code"))]
    HttpStatus { url: String, status: u16 },

    #[snafu(display("the recipe template for recipe type '{recipe}' failed to be rendered"))]
    Render {
        source: handlebars::RenderError,
        recipe: String,
        template: String,
    },

    #[snafu(display(
        "sha256 checksum mismatch for recipe template: expected {expected}, actual {actual}"
    ))]
    ChecksumMismatch { expected: String, actual: String },

    #[snafu(display("failed to read cached recipe template"))]
    ReadCache {
        source: crate::package::RecipeCacheError,
    },

    #[snafu(display("failed to cache recipe template"))]
    CacheRecipe {
        source: crate::package::RecipeCacheError,
    },

    #[snafu(display("failed to acquire lock on package cache"))]
    LockCache { source: crate::fs::lock::LockError },

    #[snafu(display("failed to resolve git tag '{tag}' for recipe"))]
    ResolveGitTag { source: reqwest::Error, tag: String },

    #[snafu(display("failed to parse git tag response for '{tag}'"))]
    ParseGitTag { tag: String },
}

impl Handlebars {
    async fn resolve_impl(
        &self,
        recipe: &Recipe,
    ) -> Result<(BuildSteps, SyncSteps), HandlebarsError> {
        // Determine the template source
        let tmpl = match &recipe.recipe_type {
            RecipeType::File(path) => TemplateSource::LocalPath(Path::new(&path).into()),
            RecipeType::Url(url) => TemplateSource::RemoteUrl(url.to_owned()),
            RecipeType::Registry {
                name,
                recipe,
                version,
            } => TemplateSource::Registry(name.to_owned(), recipe.to_owned(), version.to_owned()),
        };

        // Retrieve the template, using cache for remote/registry sources
        let tmpl = match tmpl {
            TemplateSource::LocalPath(path) => {
                let bytes = read(&path).context(ReadFileSnafu)?;
                if let Some(expected) = &recipe.sha256 {
                    verify_checksum(&bytes, expected)?;
                }
                parse_bytes_to_string(bytes)?
            }

            TemplateSource::RemoteUrl(u) => {
                self.fetch_remote_template(&u, recipe.sha256.as_deref())
                    .await?
            }

            // TMP(or.ricon): Temporarily hardcode a dfinity registry
            TemplateSource::Registry(registry, recipe_name, version) => {
                if registry != "dfinity" {
                    panic!("only the dfinity registry is currently supported");
                }

                let release_tag = format!("{recipe_name}-{version}");

                // Check cache
                let maybe_cached = self
                    .pkg_cache
                    .with_read(async |r| {
                        read_cached_recipe(r, &release_tag).context(ReadCacheSnafu)
                    })
                    .await
                    .context(LockCacheSnafu)?;
                if let Some(cached) = maybe_cached? {
                    debug!("Using cached recipe template for {release_tag}");
                    parse_bytes_to_string(cached)?
                } else {
                    // Download the template
                    let url = format!(
                        "https://github.com/dfinity/icp-cli-recipes/releases/download/{release_tag}/recipe.hbs"
                    );
                    let bytes = self.fetch_remote_bytes(&url).await?;

                    if let Some(expected) = &recipe.sha256 {
                        verify_checksum(&bytes, expected)?;
                    }

                    // Resolve the git tag to a commit SHA for caching
                    let git_sha = self
                        .resolve_git_tag_sha("dfinity", "icp-cli-recipes", &release_tag)
                        .await?;

                    // Cache the template keyed by the git SHA
                    self.pkg_cache
                        .with_write(async |w| {
                            cache_recipe(w, &release_tag, &git_sha, &bytes)
                                .context(CacheRecipeSnafu)
                        })
                        .await
                        .context(LockCacheSnafu)??;

                    parse_bytes_to_string(bytes)?
                }
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
            .context(RenderSnafu {
                recipe: recipe.recipe_type.clone(),
                template: tmpl.to_owned(),
            })?;

        // Read the rendered YAML canister manifest
        // Recipes can only render build/sync
        #[derive(Deserialize)]
        struct BuildSyncHelper {
            build: BuildSteps,
            #[serde(default)]
            sync: SyncSteps,
        }

        let insts = serde_yaml::from_str::<BuildSyncHelper>(&out);
        match insts {
            Ok(helper) => Ok((helper.build, helper.sync)),
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
        }
    }

    /// Fetch raw bytes from a remote URL.
    async fn fetch_remote_bytes(&self, url: &str) -> Result<Vec<u8>, HandlebarsError> {
        let u = Url::from_str(url).context(UrlParseSnafu)?;
        debug!("Requesting template from: {u}");

        let resp = self
            .http_client
            .execute(Request::new(Method::GET, u.clone()))
            .await
            .context(HttpRequestSnafu)?;

        if !resp.status().is_success() {
            return HttpStatusSnafu {
                url: u.to_string(),
                status: resp.status().as_u16(),
            }
            .fail();
        }

        Ok(resp.bytes().await.context(HttpRequestSnafu)?.to_vec())
    }

    /// Fetch a remote template, verifying checksum if provided.
    async fn fetch_remote_template(
        &self,
        url: &str,
        sha256: Option<&str>,
    ) -> Result<String, HandlebarsError> {
        let bytes = self.fetch_remote_bytes(url).await?;
        if let Some(expected) = sha256 {
            verify_checksum(&bytes, expected)?;
        }
        parse_bytes_to_string(bytes)
    }

    /// Resolve a GitHub release tag to its underlying git commit SHA.
    async fn resolve_git_tag_sha(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
    ) -> Result<String, HandlebarsError> {
        let ref_response = self
            .github_api_get(
                &format!("https://api.github.com/repos/{owner}/{repo}/git/ref/tags/{tag}"),
                tag,
            )
            .await?;

        let obj_type = ref_response["object"]["type"]
            .as_str()
            .ok_or_else(|| ParseGitTagSnafu { tag }.build())?;
        let obj_sha = ref_response["object"]["sha"]
            .as_str()
            .ok_or_else(|| ParseGitTagSnafu { tag }.build())?;

        match obj_type {
            // Lightweight tag — points directly at the commit
            "commit" => Ok(obj_sha.to_owned()),
            // Annotated tag — dereference through the tag object to get the commit
            "tag" => {
                let tag_response = self
                    .github_api_get(
                        &format!("https://api.github.com/repos/{owner}/{repo}/git/tags/{obj_sha}"),
                        tag,
                    )
                    .await?;
                tag_response["object"]["sha"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| ParseGitTagSnafu { tag }.build())
            }
            _ => ParseGitTagSnafu { tag }.fail(),
        }
    }

    /// Make an authenticated GET request to the GitHub API.
    async fn github_api_get(
        &self,
        url: &str,
        tag: &str,
    ) -> Result<serde_json::Value, HandlebarsError> {
        let mut req = self.http_client.get(url).header("User-Agent", "icp-cli");
        if let Ok(token) = std::env::var("ICP_CLI_GITHUB_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
            .await
            .context(ResolveGitTagSnafu { tag })?
            .error_for_status()
            .context(ResolveGitTagSnafu { tag })?
            .json()
            .await
            .context(ResolveGitTagSnafu { tag })
    }
}

#[async_trait]
impl Resolve for Handlebars {
    async fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        self.resolve_impl(recipe)
            .await
            .context(super::HandlebarsSnafu)
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
fn verify_checksum(bytes: &[u8], expected: &str) -> Result<(), HandlebarsError> {
    let actual = hex::encode({
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize()
    });

    if actual != expected {
        return ChecksumMismatchSnafu {
            expected: expected.to_string(),
            actual,
        }
        .fail();
    }

    Ok(())
}

/// Helper function to parse bytes into a UTF-8 string
fn parse_bytes_to_string(bytes: Vec<u8>) -> Result<String, HandlebarsError> {
    String::from_utf8(bytes).context(DecodeUtf8Snafu)
}
