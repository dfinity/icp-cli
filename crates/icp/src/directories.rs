//! Directory management for ICP CLI.
//!
//! This module provides utilities for determining and managing directory paths
//! used by the ICP CLI tool. It handles both standard system directories and
//! custom overrides, primarily for storing user data like identities and cache.

use crate::prelude::*;
use directories::ProjectDirs;

/// Trait for accessing ICP CLI directories.
pub trait Access: Sync + Send {
    /// Returns the path to the identity directory.
    fn identity(&self) -> PathBuf;

    /// Returns the path to the port descriptors cache directory.
    fn port_descriptor(&self) -> PathBuf;
}

/// Inner structure holding data and cache directory paths.
///
/// This struct is used when the directories are determined using standard
/// system conventions via the `directories` crate.
#[derive(Debug, Clone)]
pub struct DirectoriesInner {
    /// Path to the data directory for storing user data.
    data: PathBuf,
    /// Path to the cache directory for temporary files.
    cache: PathBuf,
}

/// Implementation for creating `DirectoriesInner` from `ProjectDirs`.
impl DirectoriesInner {
    /// Creates a `DirectoriesInner` from a `ProjectDirs` instance.
    ///
    /// This method extracts the data and cache directory paths from the
    /// `ProjectDirs` and converts them to UTF-8 validated paths.
    ///
    /// # Errors
    /// Returns `FromPathBufError` if the paths contain non-UTF-8 characters.
    pub fn from_dirs(dirs: ProjectDirs) -> Result<Self, FromPathBufError> {
        Ok(Self {
            data: dirs.data_dir().to_owned().try_into()?,
            cache: dirs.cache_dir().to_owned().try_into()?,
        })
    }
}

/// Enumeration representing the directory configuration.
///
/// This enum allows for two modes of directory management:
/// - Standard: Uses system-standard directories determined by the `directories` crate.
/// - Overridden: Uses a custom base path, typically set via the `ICP_HOME` environment variable.
#[derive(Debug, Clone)]
pub enum Directories {
    /// Standard directories based on system conventions.
    Standard(DirectoriesInner),
    /// Custom directory path override.
    Overridden(PathBuf),
}

/// Errors that can occur when working with directories.
#[derive(Debug, thiserror::Error)]
pub enum DirectoriesError {
    /// Failed to locate the user's home directory.
    #[error("home directory could not be located")]
    LocateHome,

    /// Directory paths contain non-UTF-8 characters.
    #[error("user directories are non-UTF-8")]
    Utf8(#[from] FromPathBufError),
}

/// Implementation of directory creation and management.
impl Directories {
    /// Creates a new `Directories` instance.
    ///
    /// This method first checks for the `ICP_HOME` environment variable to allow
    /// overriding the default directory location. If not set, it uses standard
    /// system directories via the `directories` crate for the DFINITY organization
    /// and ICP CLI application.
    ///
    /// # Returns
    /// - `Ok(Directories::Overridden(_))` if `ICP_HOME` is set
    /// - `Ok(Directories::Standard(_))` using system directories
    /// - `Err(DirectoriesError)` if directories cannot be determined
    pub fn new() -> Result<Self, DirectoriesError> {
        // Allow overriding home directory
        if let Ok(v) = std::env::var("ICP_HOME") {
            return Ok(Self::Overridden(v.into()));
        }

        let dirs = ProjectDirs::from(
            "org.dfinity", // qualifier
            "",            // organization
            "icp-cli",     // application
        )
        .ok_or(DirectoriesError::LocateHome)?;

        // Convert to utf8 paths
        let dirs = DirectoriesInner::from_dirs(dirs)?;

        Ok(Self::Standard(dirs))
    }
}

/// Implementation providing access to specific directory paths.
impl Directories {
    /// Returns the base data directory path.
    ///
    /// For standard directories, this is the system data directory.
    /// For overridden directories, this is the custom path.
    fn data(&self) -> PathBuf {
        match self {
            Self::Standard(dirs) => dirs.data.clone(),
            Self::Overridden(path) => path.clone(),
        }
    }

    /// Returns the base cache directory path.
    ///
    /// For standard directories, this is the system cache directory.
    /// For overridden directories, this is the custom path.
    fn cache(&self) -> PathBuf {
        match self {
            Self::Standard(dirs) => dirs.cache.clone(),
            Self::Overridden(path) => path.clone(),
        }
    }
}

/// Implementation of Access trait for Directories.
impl Access for Directories {
    /// Returns the path to the identity directory.
    ///
    /// This directory stores user identity files, keys, and related data.
    fn identity(&self) -> PathBuf {
        self.data().join("identity")
    }

    /// Returns the path to the port descriptors cache directory.
    ///
    /// This directory caches information about network ports used by canisters.
    fn port_descriptor(&self) -> PathBuf {
        self.cache().join("port-descriptors")
    }
}
