use std::{str::FromStr, string::FromUtf8Error};

use async_trait::async_trait;
use icp_deploy_canister::sync_exec::StepProgress;
use reqwest::{Method, Request, Url};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tracing::debug;
use url::ParseError;

use crate::{
    fs::read,
    manifest::recipe::{Recipe, RecipeType},
    package::{
        PackageCache, cache_registry_recipe, cache_uri_recipe, read_cached_registry_recipe,
        read_cached_uri_recipe,
    },
    prelude::*,
};

use super::{FetchedRecipe, RemoteResourceResolve, ResolveError};
use crate::manifest::adapter::prebuilt::SourceField;

/// Fetches recipe templates and plugin wasms over HTTP, caching downloads in the
/// package cache. Template *rendering* is the library's job
/// ([`icp_deploy_canister::canister::recipe::render_recipe`]); this only produces
/// the raw template text, and defers caching a download until the caller reports
/// that it rendered (see [`FetchedRecipe`]).
pub struct ResourceResolver {
    /// Http client for fetching remote recipe templates
    pub http_client: reqwest::Client,
    /// Package cache for caching downloaded recipe templates
    pub pkg_cache: PackageCache,
}

enum TemplateSource {
    LocalPath(PathBuf),
    RemoteUrl(String),

    /// Template originating in a remote registry, e.g `@dfinity/rust@v1.0.2`
    Registry(String, String, String),
}

#[derive(Debug, Snafu)]
pub enum RecipeFetchError {
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
}

impl ResourceResolver {
    /// Fetch a recipe's Handlebars template text: read a local file, or fetch a
    /// remote URL or registry recipe (serving it from the package cache when
    /// possible). Verifies `sha256` when set. A fresh download is *not* cached
    /// here — [`Self::cache_recipe`] does that once the caller has rendered it.
    async fn fetch_recipe(&self, recipe: &Recipe) -> Result<FetchedRecipe, RecipeFetchError> {
        // Retrieve the template, using cache for remote/registry sources
        let (tmpl, deferred) = match &template_source(&recipe.recipe_type) {
            TemplateSource::LocalPath(path) => {
                let bytes = read(path).context(ReadFileSnafu)?;
                (parse_bytes_to_string(bytes)?, false)
            }

            TemplateSource::RemoteUrl(u) => {
                // Check cache
                let maybe_cached = self
                    .pkg_cache
                    .with_read(async |r| {
                        read_cached_uri_recipe(r, u, recipe.sha256.as_deref())
                            .context(ReadCacheSnafu)
                    })
                    .await
                    .context(LockCacheSnafu)?;
                if let Some(cached) = maybe_cached? {
                    debug!("Using cached recipe template for {u}");
                    (parse_bytes_to_string(cached)?, false)
                } else {
                    // Download the template
                    let tmpl = self.fetch_remote_bytes(u).await?;
                    (parse_bytes_to_string(tmpl)?, true)
                }
            }

            // TMP(or.ricon): Temporarily hardcode a dfinity registry
            TemplateSource::Registry(registry, recipe_name, version) => {
                if registry != "dfinity" {
                    panic!("only the dfinity registry is currently supported");
                }

                let package = format!("@{registry}/{recipe_name}");
                let release_tag = format!("{recipe_name}-{version}");

                // Check cache
                let maybe_cached = self
                    .pkg_cache
                    .with_read(async |r| {
                        read_cached_registry_recipe(r, &package, version).context(ReadCacheSnafu)
                    })
                    .await
                    .context(LockCacheSnafu)?;
                if let Some(cached) = maybe_cached? {
                    debug!("Using cached recipe template for {package}@{version}");
                    (parse_bytes_to_string(cached)?, false)
                } else {
                    // Download the template
                    let url = format!(
                        "https://github.com/dfinity/icp-cli-recipes/releases/download/{release_tag}/recipe.hbs"
                    );
                    let bytes = self.fetch_remote_bytes(&url).await?;

                    (parse_bytes_to_string(bytes)?, true)
                }
            }
        };

        if let Some(sha256) = &recipe.sha256 {
            verify_checksum(tmpl.as_bytes(), sha256)?;
        }

        Ok(FetchedRecipe {
            template: tmpl,
            deferred,
        })
    }

    /// Cache a template downloaded by [`Self::fetch_recipe`]. Called only after
    /// the caller has rendered it, so a malformed response never becomes the
    /// entry that later project loads reuse.
    async fn cache_recipe(&self, recipe: &Recipe, tmpl: &str) -> Result<(), RecipeFetchError> {
        let hash = hex::encode(Sha256::digest(tmpl.as_bytes()));
        match template_source(&recipe.recipe_type) {
            TemplateSource::LocalPath(_) => unreachable!("local files are never cached"),
            TemplateSource::RemoteUrl(u) => {
                self.pkg_cache
                    .with_write(async |w| {
                        cache_uri_recipe(w, &u, &hash, tmpl.as_bytes()).context(CacheRecipeSnafu)
                    })
                    .await
                    .context(LockCacheSnafu)??;
            }
            TemplateSource::Registry(registry, recipe_name, version) => {
                let package = format!("@{registry}/{recipe_name}");
                self.pkg_cache
                    .with_write(async |w| {
                        cache_registry_recipe(w, &package, &version, &hash, tmpl.as_bytes())
                            .context(CacheRecipeSnafu)
                    })
                    .await
                    .context(LockCacheSnafu)??;
            }
        }
        Ok(())
    }

    /// Fetch raw bytes from a remote URL.
    async fn fetch_remote_bytes(&self, url: &str) -> Result<Vec<u8>, RecipeFetchError> {
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
}

#[async_trait]
impl RemoteResourceResolve for ResourceResolver {
    async fn resolve_recipe(&self, recipe: &Recipe) -> Result<FetchedRecipe, ResolveError> {
        self.fetch_recipe(recipe)
            .await
            .map_err(|source| ResolveError::Resolve {
                source: Box::new(source),
            })
    }

    async fn commit_recipe(
        &self,
        recipe: &Recipe,
        fetched: &FetchedRecipe,
    ) -> Result<(), ResolveError> {
        if !fetched.deferred {
            return Ok(());
        }
        self.cache_recipe(recipe, &fetched.template)
            .await
            .map_err(|source| ResolveError::Resolve {
                source: Box::new(source),
            })
    }

    async fn resolve_wasm(
        &self,
        source: &SourceField,
        base_dir: &Path,
        sha256: Option<&str>,
        progress: Option<&dyn StepProgress>,
    ) -> Result<PathBuf, ResolveError> {
        crate::canister::wasm::resolve(source, base_dir, sha256, progress, &self.pkg_cache)
            .await
            .map_err(|source| ResolveError::ResolveWasm {
                source: Box::new(source),
            })
    }
}

/// Classify where a recipe's template comes from.
fn template_source(recipe_type: &RecipeType) -> TemplateSource {
    match recipe_type {
        RecipeType::File(path) => TemplateSource::LocalPath(Path::new(&path).into()),
        RecipeType::Url(url) => TemplateSource::RemoteUrl(url.to_owned()),
        RecipeType::Registry {
            name,
            recipe,
            version,
        } => TemplateSource::Registry(name.to_owned(), recipe.to_owned(), version.to_owned()),
    }
}

/// Helper function to verify sha256 checksum of recipe template bytes
fn verify_checksum(bytes: &[u8], expected: &str) -> Result<(), RecipeFetchError> {
    let actual = hex::encode(Sha256::digest(bytes));
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
fn parse_bytes_to_string(bytes: Vec<u8>) -> Result<String, RecipeFetchError> {
    String::from_utf8(bytes).context(DecodeUtf8Snafu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::recipe::{Recipe, RecipeType};

    /// A local recipe file is read back verbatim (rendering is the library's job).
    #[tokio::test]
    async fn local_recipe_is_read_verbatim() {
        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let tmpl_path = tmp.path().join("recipe.hbs");
        let body = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "build {{_.canister.name}}"
        "#};
        std::fs::write(&tmpl_path, body).unwrap();

        let resolver = ResourceResolver {
            http_client: reqwest::Client::new(),
            pkg_cache: PackageCache::new(tmp.path().join("pkg")).unwrap(),
        };
        let recipe = Recipe {
            recipe_type: RecipeType::File(tmpl_path.to_string()),
            configuration: Default::default(),
            sha256: None,
        };

        let fetched = resolver.fetch_recipe(&recipe).await.unwrap();
        assert_eq!(fetched.template, body);
        assert!(!fetched.deferred, "a local file has nothing to cache");
    }

    /// A sha256 that does not match the file contents is rejected.
    #[tokio::test]
    async fn checksum_mismatch_is_rejected() {
        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let tmpl_path = tmp.path().join("recipe.hbs");
        std::fs::write(&tmpl_path, "build:\n  steps: []\n").unwrap();

        let resolver = ResourceResolver {
            http_client: reqwest::Client::new(),
            pkg_cache: PackageCache::new(tmp.path().join("pkg")).unwrap(),
        };
        let recipe = Recipe {
            recipe_type: RecipeType::File(tmpl_path.to_string()),
            configuration: Default::default(),
            sha256: Some("00".repeat(32)),
        };

        assert!(matches!(
            resolver.fetch_recipe(&recipe).await,
            Err(RecipeFetchError::ChecksumMismatch { .. })
        ));
    }
}
