//! Installed extensions: where they live on this machine, and getting hold
//! of one to install.
//!
//! Each extension has a directory of its own under
//! [`crate::directories::Access::extensions`], named after the command the
//! user installed it as (which need not be the name the extension gives
//! itself, so one component can be installed twice, as `cyclepay` and
//! `cyclepay-beta`):
//!
//! - `<sha256>.wasm` — the component, named by its hash;
//! - `installed.json` — an [`Installed`]: where the component came from, the
//!   sha256 it had when the user approved it, and what it declared. That hash
//!   is checked before every run, so the component cannot change underneath
//!   an approval.
//!
//! Both are written under a temporary name and renamed into place, so a
//! reader never sees either half-written. Since components are named by their
//! hash, two installs racing under one name each write a component of their
//! own, and whichever record lands last names its own component: the pair a
//! record and its component make is never mixed.

use icp_extension::Info;
use icp_project::fs::{self, json};
use icp_project::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use url::Url;

const INSTALLED: &str = "installed.json";

/// An installed extension, as recorded at install.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installed {
    /// The command it is installed as, `icp <name>`, chosen by the user.
    pub name: String,
    /// Where it was installed from: a URL, or a local path.
    pub source: String,
    /// Hex sha256 of the component the user approved.
    pub sha256: String,
    /// Where the component is kept.
    pub wasm: PathBuf,
    /// When it was installed, RFC 3339.
    pub installed_at: String,
    /// What the extension declared, its own name and version among it.
    pub info: Info,
}

#[derive(Debug, Snafu)]
pub enum ExtensionError {
    #[snafu(transparent)]
    Json { source: json::Error },

    #[snafu(transparent)]
    Io { source: fs::IoError },

    #[snafu(transparent)]
    Rename { source: fs::RenameError },

    #[snafu(display("failed to set up an HTTP client to download the extension"))]
    HttpClient { source: reqwest::Error },

    #[snafu(display("no extension named '{name}' is installed"))]
    NotInstalled { name: String },

    #[snafu(display(
        "'{name}' is not a valid command name: use lowercase letters, digits and '-', \
         starting with a letter"
    ))]
    InvalidName { name: String },

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

/// Check a name to install an extension as: the rule `icp_extension` holds
/// an extension's own name to, which also keeps it a plain directory name.
pub fn check_name(name: &str) -> Result<(), ExtensionError> {
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    ensure!(valid, InvalidNameSnafu { name });
    Ok(())
}

/// Where the component with `sha256` of the extension installed as `name` is
/// kept.
pub fn wasm_path(dir: &Path, name: &str, sha256: &str) -> PathBuf {
    dir.join(name).join(format!("{sha256}.wasm"))
}

/// The path a file is written to before it is renamed to `path`: this
/// process's own, so that two writers do not share one.
fn temporary(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
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
    installed.sort_by(|a, b| a.name.cmp(&b.name));
    installed
}

/// The extension installed as `name`.
pub fn get(dir: &Path, name: &str) -> Result<Installed, ExtensionError> {
    check_name(name)?;
    let path = dir.join(name).join(INSTALLED);
    ensure!(path.is_file(), NotInstalledSnafu { name });
    Ok(json::load(&path)?)
}

/// The component of an installed extension, checked against the hash
/// approved at install.
pub fn wasm(installed: &Installed) -> Result<Vec<u8>, ExtensionError> {
    let name = &installed.name;
    let path = installed.wasm.clone();
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

/// Whether the component kept for `installed` is still the one approved.
pub fn intact(installed: &Installed) -> bool {
    fs::read(&installed.wasm).is_ok_and(|wasm| sha256(&wasm) == installed.sha256)
}

/// Install (or replace) an extension under `dir`, its component at
/// `installed.wasm` (see [`wasm_path`]).
pub fn install(dir: &Path, wasm: &[u8], installed: &Installed) -> Result<(), ExtensionError> {
    check_name(&installed.name)?;
    let home = dir.join(&installed.name);
    fs::create_dir_all(&home)?;
    let previous = get(dir, &installed.name).ok();

    let staged = temporary(&installed.wasm);
    fs::write(&staged, wasm)?;
    fs::rename(&staged, &installed.wasm)?;
    let record = home.join(INSTALLED);
    let staged = temporary(&record);
    json::save(&staged, installed)?;
    fs::rename(&staged, &record)?;

    // The component this replaced. A racing install never needs it: only one
    // that read this install's record as its previous one would remove this
    // install's component, and its own record then lands after this one.
    if let Some(previous) = previous
        && previous.wasm != installed.wasm
    {
        let _ = fs::remove_file(&previous.wasm);
    }
    Ok(())
}

/// Remove the extension installed as `name`.
pub fn uninstall(dir: &Path, name: &str) -> Result<(), ExtensionError> {
    check_name(name)?;
    let home = dir.join(name);
    ensure!(home.join(INSTALLED).is_file(), NotInstalledSnafu { name });
    fs::remove_dir_all(&home)?;
    Ok(())
}

/// Download a component. Only https is accepted: nothing else proves where
/// the bytes came from.
pub async fn download(url: &Url) -> Result<Vec<u8>, ExtensionError> {
    ensure!(url.scheme() == "https", NotHttpsSnafu { url: url.as_str() });
    // https_only also refuses a redirect to plain http.
    let client = Client::builder()
        .https_only(true)
        .build()
        .context(HttpClientSnafu)?;
    let response = client
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

    fn installed(dir: &Path, name: &str, wasm: &[u8]) -> Installed {
        Installed {
            name: name.into(),
            source: "test".into(),
            sha256: sha256(wasm),
            wasm: wasm_path(dir, name, &sha256(wasm)),
            installed_at: "2026-10-26T00:00:00Z".into(),
            info: Info {
                name: "demo".into(),
                version: "0.1.0".into(),
                summary: String::new(),
                commands: vec![],
                canisters: vec![],
            },
        }
    }

    #[test]
    fn install_list_and_uninstall() {
        let tmp = camino_tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(list(dir).is_empty());

        // One extension, installed under two names.
        install(dir, b"b", &installed(dir, "bravo", b"b")).unwrap();
        install(dir, b"a", &installed(dir, "alpha", b"a")).unwrap();
        let names: Vec<_> = list(dir).into_iter().map(|i| i.name).collect();
        assert_eq!(names, ["alpha", "bravo"]);

        let alpha = get(dir, "alpha").unwrap();
        assert_eq!(wasm(&alpha).unwrap(), b"a");
        assert!(intact(&alpha));

        uninstall(dir, "alpha").unwrap();
        assert!(matches!(
            get(dir, "alpha"),
            Err(ExtensionError::NotInstalled { .. })
        ));
        assert!(matches!(
            uninstall(dir, "alpha"),
            Err(ExtensionError::NotInstalled { .. })
        ));
        assert_eq!(get(dir, "bravo").unwrap().info.name, "demo");
    }

    #[test]
    fn a_changed_component_is_refused() {
        let tmp = camino_tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let record = installed(dir, "alpha", b"a");
        install(dir, b"a", &record).unwrap();
        fs::write(&record.wasm, b"evil").unwrap();
        assert!(matches!(
            wasm(&record),
            Err(ExtensionError::Tampered { .. })
        ));
        assert!(!intact(&record));
    }

    #[test]
    fn a_reinstall_switches_to_its_own_component() {
        let tmp = camino_tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let first = installed(dir, "alpha", b"one");
        let second = installed(dir, "alpha", b"two");
        assert_ne!(first.wasm, second.wasm);

        install(dir, b"one", &first).unwrap();
        install(dir, b"two", &second).unwrap();
        let current = get(dir, "alpha").unwrap();
        assert_eq!(current.wasm, second.wasm);
        assert_eq!(wasm(&current).unwrap(), b"two");
        // The replaced component is gone; nothing is left half-written.
        assert!(!first.wasm.exists());
        let mut files: Vec<_> = fs::read_dir(&dir.join("alpha")).unwrap();
        files.sort();
        assert_eq!(
            files,
            [second.wasm.clone(), dir.join("alpha").join(INSTALLED)]
        );
    }

    #[test]
    fn a_name_that_is_not_a_plain_directory_name_is_refused() {
        let tmp = camino_tempfile::tempdir().unwrap();
        for name in ["../x", "a/b", "", "Upper", "-x"] {
            assert!(
                matches!(
                    get(tmp.path(), name),
                    Err(ExtensionError::InvalidName { .. })
                ),
                "{name}"
            );
        }
    }
}
