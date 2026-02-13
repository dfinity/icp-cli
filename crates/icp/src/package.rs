use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use crate::{
    fs::lock::{DirectoryStructureLock, LRead, LWrite, LockError, PathsAccess},
    prelude::*,
};

pub struct PackageCachePaths {
    root: PathBuf,
}

impl PackageCachePaths {
    pub fn launcher_dir(&self) -> PathBuf {
        self.root.join("network-launcher")
    }
    pub fn recipes_dir(&self) -> PathBuf {
        self.root.join("recipes")
    }
    pub fn project_templates_dir(&self) -> PathBuf {
        self.root.join("project-templates")
    }
    pub fn canisters_dir(&self) -> PathBuf {
        self.root.join("canisters")
    }
    pub fn launcher_version(&self, version: &str) -> PathBuf {
        self.launcher_dir().join(version)
    }
    pub fn canister_sha(&self, sha: &str) -> CanisterCache {
        CanisterCache {
            dir: self.canisters_dir().join(sha),
        }
    }
    pub fn recipe_sha(&self, sha: &str) -> RecipeCache {
        RecipeCache {
            dir: self.recipes_dir().join(sha),
        }
    }
    pub fn manifest(&self) -> PathBuf {
        self.root.join("manifest.json")
    }
}

pub struct CanisterCache {
    dir: PathBuf,
}

impl CanisterCache {
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn wasm(&self) -> PathBuf {
        self.dir.join("canister.wasm")
    }
    pub fn atime(&self) -> PathBuf {
        self.dir.join(".atime")
    }
}

pub struct RecipeCache {
    dir: PathBuf,
}

impl RecipeCache {
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn template(&self) -> PathBuf {
        self.dir.join("recipe.hbs")
    }
    pub fn atime(&self) -> PathBuf {
        self.dir.join(".atime")
    }
}

pub fn read_cached_prebuilt(
    cache: LRead<&PackageCachePaths>,
    sha: &str,
) -> Result<Option<Vec<u8>>, crate::fs::IoError> {
    let cache_path = cache.canister_sha(sha);
    let cache_wasm_path = cache_path.wasm();
    if cache_wasm_path.exists() {
        let wasm = crate::fs::read(&cache_wasm_path)?;
        _ = crate::fs::write(&cache_path.atime(), b"");
        Ok(Some(wasm))
    } else {
        Ok(None)
    }
}

pub fn cache_prebuilt(
    cache: LWrite<&PackageCachePaths>,
    sha: &str,
    wasm: &[u8],
) -> Result<(), crate::fs::IoError> {
    let cache_path = cache.canister_sha(sha);
    let cache_wasm_path = cache_path.wasm();
    if !cache_wasm_path.exists() {
        crate::fs::create_dir_all(cache_path.dir())?;
        crate::fs::write(&cache_wasm_path, wasm)?;
        _ = crate::fs::write(&cache_path.atime(), b"");
    }
    Ok(())
}

/// Read a cached recipe template by recipe and version (e.g., `@dfinity/rust`, `v1.0.2`).
/// Resolves the version to a git SHA via the package manifest, then reads
/// the cached template from `recipes/{sha}/recipe.hbs`.
pub fn read_cached_registry_recipe(
    cache: LRead<&PackageCachePaths>,
    recipe: &str,
    version: &str,
) -> Result<Option<Vec<u8>>, RecipeCacheError> {
    assert!(recipe.starts_with('@'));
    let Some(sha) =
        get_tag(cache, &format!("recipe{recipe}"), version).context(LoadRecipeTagSnafu)?
    else {
        return Ok(None);
    };
    read_cached_recipe(cache, &sha)
}

pub fn read_cached_uri_recipe(
    cache: LRead<&PackageCachePaths>,
    uri: &str,
    sha2: Option<&str>,
) -> Result<Option<Vec<u8>>, RecipeCacheError> {
    assert!(uri.starts_with("http://") || uri.starts_with("https://"));
    let Some(latest) =
        get_tag(cache, &format!("uri+{uri}"), "latest").context(LoadRecipeTagSnafu)?
    else {
        return Ok(None);
    };
    if let Some(sha2) = sha2
        && sha2 != latest
    {
        // the content at this URL has clearly changed, but we may have a cached recipe for the old version
        read_cached_recipe(cache, sha2)
    } else {
        read_cached_recipe(cache, &latest)
    }
}

pub fn read_cached_recipe(
    cache: LRead<&PackageCachePaths>,
    cache_key: &str,
) -> Result<Option<Vec<u8>>, RecipeCacheError> {
    let cache_path = cache.recipe_sha(cache_key);
    let template_path = cache_path.template();
    if template_path.exists() {
        let template = crate::fs::read(&template_path).context(RecipeCacheIoSnafu)?;
        _ = crate::fs::write(&cache_path.atime(), b"");
        Ok(Some(template))
    } else {
        Ok(None)
    }
}

/// Cache a recipe template. `recipe` is the registry-qualified name (e.g., `@dfinity/rust`),
/// `version` is the recipe version (e.g., `v1.0.2`), and `sha2` is the hash of the recipe
/// (not the git commit sha). Stores the version→SHA mapping in the package manifest and
/// writes the template to `recipes/{sha2}/recipe.hbs`.
pub fn cache_registry_recipe(
    cache: LWrite<&PackageCachePaths>,
    recipe: &str,
    version: &str,
    sha2: &str,
    template: &[u8],
) -> Result<(), RecipeCacheError> {
    assert!(recipe.starts_with('@'));
    set_tag(cache, &format!("recipe{recipe}"), sha2, version).context(SaveRecipeTagSnafu)?;
    cache_recipe(cache, sha2, template)
}

/// Cache a recipe template from a URL. Stores the version→SHA mapping in the package manifest
/// and writes the template to `recipes/{sha2}/recipe.hbs`.
pub fn cache_uri_recipe(
    cache: LWrite<&PackageCachePaths>,
    uri: &str,
    sha2: &str,
    template: &[u8],
) -> Result<(), RecipeCacheError> {
    assert!(uri.starts_with("http://") || uri.starts_with("https://"));
    set_tag(cache, &format!("uri+{uri}"), sha2, "latest").context(SaveRecipeTagSnafu)?;
    cache_recipe(cache, sha2, template)
}

pub fn cache_recipe(
    cache: LWrite<&PackageCachePaths>,
    cache_key: &str,
    template: &[u8],
) -> Result<(), RecipeCacheError> {
    let cache_path = cache.recipe_sha(cache_key);
    let template_path = cache_path.template();
    if !template_path.exists() {
        crate::fs::create_dir_all(cache_path.dir()).context(RecipeCacheIoSnafu)?;
        crate::fs::write(&template_path, template).context(RecipeCacheIoSnafu)?;
        _ = crate::fs::write(&cache_path.atime(), b"");
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RecipeCacheError {
    #[snafu(display("failed to load recipe cache tag"))]
    LoadRecipeTag { source: crate::fs::json::Error },

    #[snafu(display("failed to save recipe cache tag"))]
    SaveRecipeTag { source: crate::fs::json::Error },

    #[snafu(display("failed to read or write recipe cache file"))]
    RecipeCacheIo { source: crate::fs::IoError },
}

pub type PackageCache = DirectoryStructureLock<PackageCachePaths>;

impl PackageCache {
    pub fn new(root: PathBuf) -> Result<Self, LockError> {
        DirectoryStructureLock::open_or_create(PackageCachePaths { root })
    }
}

impl PathsAccess for PackageCachePaths {
    fn lock_file(&self) -> PathBuf {
        self.root.join(".lock")
    }
}

pub fn get_tag(
    paths: LRead<&PackageCachePaths>,
    tool: &str,
    tag: &str,
) -> Result<Option<String>, crate::fs::json::Error> {
    let manifest: Manifest = crate::fs::json::load_or_default(&paths.manifest())?;
    Ok(manifest.tags.get(&format!("{tool}:{tag}")).cloned())
}

pub fn set_tag(
    paths: LWrite<&PackageCachePaths>,
    tool: &str,
    version: &str,
    tag: &str,
) -> Result<(), crate::fs::json::Error> {
    let mut manifest: Manifest = crate::fs::json::load_or_default(&paths.manifest())?;
    manifest
        .tags
        .insert(format!("{tool}:{tag}"), version.to_string());
    crate::fs::json::save(&paths.manifest(), &manifest)?;
    Ok(())
}

#[derive(Serialize, Deserialize, Default)]
struct Manifest {
    tags: HashMap<String, String>,
}
