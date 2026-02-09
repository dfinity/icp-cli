use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
    version: &str,
) -> Result<Option<String>, crate::fs::json::Error> {
    let manifest: Manifest = crate::fs::json::load_or_default(&paths.manifest())?;
    Ok(manifest.tags.get(&format!("{tool}:{version}")).cloned())
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
