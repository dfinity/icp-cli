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
    pub fn launcher_version(&self, version: &str) -> PathBuf {
        self.launcher_dir().join(version)
    }
    pub fn manifest(&self) -> PathBuf {
        self.root.join("manifest.json")
    }
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
