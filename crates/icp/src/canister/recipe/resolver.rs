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
/// the raw template text.
///
/// Whether a download is worth keeping is not known until it renders, so an
/// *unpinned* download is marked [`FetchedRecipe::deferred`] and only written to
/// the cache when the caller reports back through
/// [`commit_recipe`](RemoteResourceResolve::commit_recipe). A checksummed
/// download is cached during the fetch — the checksum already proves the bytes
/// are the ones that were asked for.
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
    /// possible). Verifies `sha256` when set.
    ///
    /// A checksummed download is cached here. An unpinned one is returned with
    /// [`FetchedRecipe::deferred`] set, for the caller to commit once it renders.
    async fn fetch_recipe(&self, recipe: &Recipe) -> Result<FetchedRecipe, RecipeFetchError> {
        // Retrieve the template, using cache for remote/registry sources. The
        // flag says whether the bytes were freshly downloaded, and so are the
        // only ones that could still need caching.
        let (tmpl, downloaded) = match &template_source(&recipe.recipe_type) {
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
            // The checksum matched, so refetching could only produce these same
            // bytes: there is nothing to gain by waiting for a render that may
            // never succeed.
            if downloaded {
                self.cache_recipe(recipe, &tmpl).await?;
            }
            return Ok(FetchedRecipe {
                template: tmpl,
                deferred: false,
            });
        }

        Ok(FetchedRecipe {
            template: tmpl,
            deferred: downloaded,
        })
    }

    /// Cache a template downloaded by [`Self::fetch_recipe`]. For an unpinned
    /// download this runs only after the caller has rendered it, so a malformed
    /// response never becomes the entry that later project loads reuse.
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
    use crate::canister::recipe::{RecipeContext, render_recipe};
    use crate::manifest::recipe::{Recipe, RecipeType};

    fn resolver(cache_dir: &Path) -> ResourceResolver {
        ResourceResolver {
            http_client: reqwest::Client::new(),
            pkg_cache: PackageCache::new(cache_dir.to_owned()).unwrap(),
        }
    }

    /// A local recipe file is read back verbatim — rendering is a later stage, so
    /// the fetched text still contains its unexpanded `{{...}}` expressions.
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

        let recipe = Recipe {
            recipe_type: RecipeType::File(tmpl_path.to_string()),
            configuration: Default::default(),
            sha256: None,
        };

        let fetched = resolver(&tmp.path().join("pkg"))
            .fetch_recipe(&recipe)
            .await
            .unwrap();
        assert_eq!(fetched.template, body);
        assert!(!fetched.deferred, "a local file has nothing to cache");
    }

    /// A sha256 that does not match the template contents is rejected.
    #[tokio::test]
    async fn checksum_mismatch_is_rejected() {
        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let tmpl_path = tmp.path().join("recipe.hbs");
        std::fs::write(&tmpl_path, "build:\n  steps: []\n").unwrap();

        let recipe = Recipe {
            recipe_type: RecipeType::File(tmpl_path.to_string()),
            configuration: Default::default(),
            sha256: Some("00".repeat(32)),
        };

        assert!(matches!(
            resolver(&tmp.path().join("pkg"))
                .fetch_recipe(&recipe)
                .await,
            Err(RecipeFetchError::ChecksumMismatch { .. })
        ));
    }

    /// A template that is valid UTF-8 but does not render.
    const UNRENDERABLE: &str = indoc::indoc! {r#"
        build:
          steps:
            - type: script
              command: "{{ never_set }}"
    "#};

    /// Serves `body` once, then 500 for every later request. A second fetch that
    /// succeeds therefore proves the cache answered it; one that fails with
    /// `HttpStatus` proves it went back to the network.
    fn serve_once_then_fail(body: &str) -> (httptest::Server, String) {
        use httptest::{Expectation, Server, matchers::*, responders::*};

        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/recipe.hbs"))
                .times(1..=10)
                .respond_with(cycle![
                    status_code(200).body(body.to_owned()),
                    status_code(500),
                    status_code(500),
                    status_code(500),
                ]),
        );
        let url = server.url("/recipe.hbs").to_string();
        (server, url)
    }

    /// True when the package cache holds a stored recipe template.
    fn cache_has_template(cache_dir: &Path) -> bool {
        let recipes = cache_dir.join("recipes");
        let Ok(entries) = std::fs::read_dir(&recipes) else {
            return false;
        };
        entries.flatten().any(|e| {
            PathBuf::try_from(e.path())
                .map(|p| p.join("recipe.hbs").exists())
                .unwrap_or(false)
        })
    }

    /// REGRESSION: an unpinned remote template must not be cached before it is
    /// known to render. Otherwise one bad response becomes sticky and every later
    /// resolution reads the bad bytes back instead of refetching — which is what
    /// the pre-split implementation did, because it cached only after rendering.
    #[tokio::test]
    async fn unpinned_download_is_not_cached_until_committed() {
        let (_server, url) = serve_once_then_fail(UNRENDERABLE);

        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let cache_dir = tmp.path().join("pkg");
        let r = resolver(&cache_dir);
        let recipe = Recipe {
            recipe_type: RecipeType::Url(url),
            configuration: Default::default(),
            sha256: None,
        };

        // Fetch succeeds and hands back a held-back cache write.
        let fetched = r.fetch_recipe(&recipe).await.expect("first fetch");
        assert!(
            fetched.deferred,
            "an unpinned download must defer its cache write"
        );

        // Rendering fails, so the caller never commits.
        let ctx = RecipeContext {
            canister_name: "c".to_owned(),
        };
        assert!(
            render_recipe(&fetched.template, &recipe, &ctx).is_err(),
            "fixture template must fail to render"
        );

        assert!(
            !cache_has_template(&cache_dir),
            "a template that never rendered must not be in the cache"
        );

        // The next resolution must go back to the network rather than serve the
        // bad bytes from cache.
        assert!(
            matches!(
                r.fetch_recipe(&recipe).await,
                Err(RecipeFetchError::HttpStatus { status: 500, .. })
            ),
            "second resolution must refetch, not read the uncommitted template back"
        );
    }

    /// Once an unpinned template renders, committing it makes it cacheable — so
    /// the deferral costs nothing for templates that are actually good.
    #[tokio::test]
    async fn committing_an_unpinned_download_caches_it() {
        let good = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "build {{_.canister.name}}"
        "#};
        let (_server, url) = serve_once_then_fail(good);

        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let cache_dir = tmp.path().join("pkg");
        let r = resolver(&cache_dir);
        let recipe = Recipe {
            recipe_type: RecipeType::Url(url),
            configuration: Default::default(),
            sha256: None,
        };

        let fetched = r.fetch_recipe(&recipe).await.expect("first fetch");
        assert!(fetched.deferred, "unpinned defers its write");
        r.commit_recipe(&recipe, &fetched).await.expect("commit");

        assert!(cache_has_template(&cache_dir));

        // Served from cache now, even though the server would answer 500.
        let again = r.fetch_recipe(&recipe).await.expect("second fetch");
        assert_eq!(again.template, fetched.template);
        assert!(!again.deferred, "a cache hit has nothing to commit");
    }

    /// A checksummed download is cached during the fetch: the checksum already
    /// proves the bytes are the ones that were asked for, and refetching would
    /// only produce the same bytes, so there is nothing to gain by waiting for a
    /// render that may never succeed.
    #[tokio::test]
    async fn checksummed_download_is_cached_eagerly() {
        let (_server, url) = serve_once_then_fail(UNRENDERABLE);

        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let cache_dir = tmp.path().join("pkg");
        let r = resolver(&cache_dir);
        let recipe = Recipe {
            recipe_type: RecipeType::Url(url),
            configuration: Default::default(),
            sha256: Some(hex::encode(Sha256::digest(UNRENDERABLE.as_bytes()))),
        };

        let fetched = r.fetch_recipe(&recipe).await.expect("first fetch");
        assert!(
            !fetched.deferred,
            "a checksummed download is cached during the fetch"
        );
        assert!(cache_has_template(&cache_dir));
    }
}
