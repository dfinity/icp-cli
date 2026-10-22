//! Installed extensions: where they live on this machine, and getting hold
//! of one to install.
//!
//! Each extension has a directory of its own under
//! [`crate::directories::Access::extensions`], named after it:
//!
//! - `extension.wasm` — the component;
//! - `installed.json` — what it declared, which the user approved, and the
//!   sha256 the component had when they did. That hash is checked before every
//!   run, so the component cannot change underneath an approval.

use icp_extension::Info;
use icp_project::fs::{self, json};
use icp_project::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use url::Url;

const WASM: &str = "extension.wasm";
const INSTALLED: &str = "installed.json";

/// An installed extension, as recorded at install.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installed {
    pub info: Info,
    /// Hex sha256 of the component the user approved.
    pub sha256: String,
    /// Where it was installed from: a URL, or a local path.
    pub source: String,
}

#[derive(Debug, Snafu)]
pub enum ExtensionError {
    #[snafu(transparent)]
    Json { source: json::Error },

    #[snafu(transparent)]
    Io { source: fs::IoError },

    #[snafu(display("no extension named '{name}' is installed"))]
    NotInstalled { name: String },

    #[snafu(display(
        "the installed extension '{name}' at {path} has changed since it was approved \
         (sha256 {actual}, approved {expected}). Reinstall it to approve it again."
    ))]
    Tampered {
        name: String,
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[snafu(display("'{url}' is not an https URL; extensions are only downloaded over https"))]
    NotHttps { url: String },

    #[snafu(display("failed to download the extension from {url}"))]
    Download { url: String, source: reqwest::Error },

    #[snafu(display("downloading the extension from {url} failed: {status}"))]
    DownloadStatus {
        url: String,
        status: reqwest::StatusCode,
    },
}

/// Hex sha256 of a component.
pub fn sha256(wasm: &[u8]) -> String {
    hex::encode(Sha256::digest(wasm))
}

/// Every installed extension, by name. A directory that holds no readable
/// record is skipped: it is not an extension anyone could run.
pub fn list(dir: &Path) -> Vec<Installed> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut installed: Vec<Installed> = entries
        .into_iter()
        .filter_map(|entry| json::load(&entry.join(INSTALLED)).ok())
        .collect();
    installed.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    installed
}

/// The installed extension named `name`.
pub fn get(dir: &Path, name: &str) -> Result<Installed, ExtensionError> {
    let path = dir.join(name).join(INSTALLED);
    ensure!(path.is_file(), NotInstalledSnafu { name });
    Ok(json::load(&path)?)
}

/// The component of an installed extension, checked against the hash
/// approved at install.
pub fn wasm(dir: &Path, installed: &Installed) -> Result<Vec<u8>, ExtensionError> {
    let name = &installed.info.name;
    let path = dir.join(name).join(WASM);
    let wasm = fs::read(&path)?;
    let actual = sha256(&wasm);
    ensure!(
        actual == installed.sha256,
        TamperedSnafu {
            name,
            path,
            expected: &installed.sha256,
            actual,
        }
    );
    Ok(wasm)
}

/// Install (or replace) an extension.
pub fn install(dir: &Path, wasm: &[u8], installed: &Installed) -> Result<(), ExtensionError> {
    let home = dir.join(&installed.info.name);
    fs::create_dir_all(&home)?;
    fs::write(&home.join(WASM), wasm)?;
    json::save(&home.join(INSTALLED), installed)?;
    Ok(())
}

/// Remove an installed extension.
pub fn uninstall(dir: &Path, name: &str) -> Result<(), ExtensionError> {
    let home = dir.join(name);
    ensure!(home.join(INSTALLED).is_file(), NotInstalledSnafu { name });
    fs::remove_dir_all(&home)?;
    Ok(())
}

/// Download a component. Only https is accepted: nothing else proves where
/// the bytes came from.
pub async fn download(url: &Url) -> Result<Vec<u8>, ExtensionError> {
    ensure!(url.scheme() == "https", NotHttpsSnafu { url: url.as_str() });
    let response = Client::new()
        .get(url.clone())
        .send()
        .await
        .context(DownloadSnafu { url: url.as_str() })?;
    let status = response.status();
    ensure!(
        status.is_success(),
        DownloadStatusSnafu {
            url: url.as_str(),
            status,
        }
    );
    let bytes = response
        .bytes()
        .await
        .context(DownloadSnafu { url: url.as_str() })?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(name: &str, wasm: &[u8]) -> Installed {
        Installed {
            info: Info {
                name: name.into(),
                version: "0.1.0".into(),
                summary: String::new(),
                commands: vec![],
                canisters: vec![],
            },
            sha256: sha256(wasm),
            source: "test".into(),
        }
    }

    #[test]
    fn install_list_and_uninstall() {
        let tmp = camino_tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(list(dir).is_empty());

        install(dir, b"b", &installed("bravo", b"b")).unwrap();
        install(dir, b"a", &installed("alpha", b"a")).unwrap();
        let names: Vec<_> = list(dir).into_iter().map(|i| i.info.name).collect();
        assert_eq!(names, ["alpha", "bravo"]);

        let alpha = get(dir, "alpha").unwrap();
        assert_eq!(wasm(dir, &alpha).unwrap(), b"a");

        uninstall(dir, "alpha").unwrap();
        assert!(matches!(
            get(dir, "alpha"),
            Err(ExtensionError::NotInstalled { .. })
        ));
        assert!(matches!(
            uninstall(dir, "alpha"),
            Err(ExtensionError::NotInstalled { .. })
        ));
    }

    #[test]
    fn a_changed_component_is_refused() {
        let tmp = camino_tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let record = installed("alpha", b"a");
        install(dir, b"a", &record).unwrap();
        fs::write(&dir.join("alpha").join(WASM), b"evil").unwrap();
        assert!(matches!(
            wasm(dir, &record),
            Err(ExtensionError::Tampered { .. })
        ));
    }
}
