//! User settings management for ICP CLI.
//!
//! This module provides utilities for loading and saving user settings.
//! Settings are stored in a dedicated directory with an adjacent lock file
//! to ensure safe concurrent access.

use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};

use crate::{
    fs::{
        json,
        lock::{DirectoryStructureLock, LRead, LWrite, LockError, PathsAccess},
    },
    prelude::*,
};

/// Paths for user settings storage.
pub struct SettingsPaths {
    dir: PathBuf,
}

impl SettingsPaths {
    /// Creates a new settings directory lock.
    pub fn new(dir: PathBuf) -> Result<SettingsDirectories, LockError> {
        DirectoryStructureLock::open_or_create(Self { dir })
    }

    /// Returns the path to the settings file.
    pub fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    /// Ensures the settings directory exists and returns the path to the settings file.
    pub fn ensure_settings_path(&self) -> Result<PathBuf, crate::fs::IoError> {
        crate::fs::create_dir_all(&self.dir)?;
        Ok(self.settings_path())
    }
}

/// Type alias for the locked settings directory structure.
pub type SettingsDirectories = DirectoryStructureLock<SettingsPaths>;

impl PathsAccess for SettingsPaths {
    fn lock_file(&self) -> PathBuf {
        self.dir.join(".lock")
    }
}

/// User settings for the ICP CLI.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Settings {
    /// Schema version for forwards compatibility.
    pub v: u32,

    /// Use Docker for the network launcher even when native mode is requested.
    #[serde(default)]
    pub autodockerize: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            v: 1,
            autodockerize: false,
        }
    }
}

impl Settings {
    /// Writes the settings to the settings file.
    pub fn write_to(&self, dirs: LWrite<&SettingsPaths>) -> Result<(), WriteSettingsError> {
        json::save(&dirs.ensure_settings_path()?, self)?;
        Ok(())
    }

    /// Loads settings from the settings file, or returns defaults if the file doesn't exist.
    pub fn load_from(dirs: LRead<&SettingsPaths>) -> Result<Self, LoadSettingsError> {
        let settings_path = dirs.settings_path();

        let settings: Self = json::load_or_default(&settings_path)?;

        ensure!(
            settings.v == 1,
            BadVersionSnafu {
                path: &settings_path
            }
        );

        Ok(settings)
    }
}

#[derive(Debug, Snafu)]
pub enum WriteSettingsError {
    #[snafu(transparent)]
    WriteJsonError { source: json::Error },

    #[snafu(transparent)]
    CreateDirectoryError { source: crate::fs::IoError },
}

#[derive(Debug, Snafu)]
pub enum LoadSettingsError {
    #[snafu(transparent)]
    LoadJsonError { source: json::Error },

    #[snafu(display("file `{path}` was modified by an incompatible new version of icp-cli"))]
    BadVersion { path: PathBuf },
}
