use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::time::{Duration, SystemTime};

use flate2::bufread::GzDecoder;
use futures::{StreamExt, TryStreamExt};
use reqwest::Client;
use snafu::{ResultExt, Snafu};
use tar::Archive;
use tracing::debug;

use crate::fs::lock::{LRead, LWrite};
use crate::package::{PackageCachePaths, get_tag, get_tag_with_updater, set_tag_with_updater};
use crate::prelude::*;

const LAUNCHER_NAME: &str = "icp-cli-network-launcher";

/// Returns the resolved version and path for the cached launcher binary.
/// For "latest", resolves the tag to the actual version (e.g. "v0.3.0").
pub fn get_cached_launcher_version(
    paths: LRead<&PackageCachePaths>,
    version: &str,
) -> Result<Option<(String, PathBuf)>, ReadCacheError> {
    let declared_version = if version == "latest" {
        let Some(version) = get_tag(paths, LAUNCHER_NAME, "latest").context(LoadTagSnafu)? else {
            return Ok(None);
        };
        version.to_owned()
    } else {
        assert!(version.starts_with('v'));
        version.to_owned()
    };
    let version_path = paths.launcher_version(&declared_version);
    if version_path.exists() {
        Ok(Some((declared_version, version_path.join(LAUNCHER_NAME))))
    } else {
        Ok(None)
    }
}

/// Like [`get_cached_launcher_version`], but for the "latest" tag also checks
/// whether the launcher was downloaded by an older CLI version, returning `None`
/// if so. Pinned versions are never considered stale.
pub fn get_cached_launcher_version_if_fresh(
    paths: LRead<&PackageCachePaths>,
    version: &str,
) -> Result<Option<(String, PathBuf)>, ReadCacheError> {
    if version == "latest" {
        let (_, updater) =
            get_tag_with_updater(paths, LAUNCHER_NAME, "latest").context(LoadTagSnafu)?;
        if is_updater_stale(updater.as_deref()) {
            return Ok(None);
        }
    }
    get_cached_launcher_version(paths, version)
}

/// Returns true if the given updater version is older than the current CLI version
/// (or if no updater version is recorded).
fn is_updater_stale(updater_version: Option<&str>) -> bool {
    let Some(updater_version) = updater_version else {
        return true;
    };
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("package versions should always be valid semver");
    let Ok(stored) = semver::Version::parse(updater_version) else {
        return true;
    };
    stored < current
}

#[derive(Debug, Snafu)]
pub enum ReadCacheError {
    #[snafu(display("failed to read package tag"))]
    LoadTag { source: crate::fs::json::Error },
}

pub async fn get_latest_launcher_version(client: &Client) -> Result<String, DownloadLauncherError> {
    let url = "https://api.github.com/repos/dfinity/icp-cli-network-launcher/releases/latest";
    let mut req = client.get(url).header("User-Agent", "icp-cli");
    if let Ok(token) = std::env::var("ICP_CLI_GITHUB_TOKEN") {
        req = req.bearer_auth(token);
    }
    let response: serde_json::Value = req
        .send()
        .await
        .context(LatestVersionFetchSnafu)?
        .error_for_status()
        .context(LatestVersionFetchSnafu)?
        .json()
        .await
        .context(LatestVersionFetchSnafu)?;
    let tag_name = response["tag_name"]
        .as_str()
        .ok_or_else(|| LatestVersionParseSnafu.build())?;
    Ok(tag_name.to_owned())
}

pub async fn download_launcher_version(
    paths: LWrite<&PackageCachePaths>,
    version_req: &str,
    client: &Client,
) -> Result<(String, PathBuf), DownloadLauncherError> {
    let pkg_version = if version_req == "latest" {
        let latest = get_latest_launcher_version(client).await?;
        set_tag_with_updater(
            paths,
            LAUNCHER_NAME,
            &latest,
            "latest",
            env!("CARGO_PKG_VERSION"),
        )
        .context(CreateTagSnafu)?;
        latest
    } else {
        assert!(version_req.starts_with('v'));
        version_req.to_owned()
    };
    let version_path = paths.launcher_version(&pkg_version);
    crate::fs::create_dir_all(&paths.launcher_dir()).context(CreateDirSnafu)?;
    let mut tmp = camino_tempfile::tempfile().context(TempFileSnafu)?;
    let tmp_write = BufWriter::new(&tmp);
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => UnsupportedArchSnafu {
            arch: other,
            supported: vec!["x86_64", "arm64"],
        }
        .fail()?,
    };
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        other => UnsupportedOsSnafu {
            os: other,
            supported: vec!["linux", "macos"],
        }
        .fail()?,
    };
    let url = format!(
        "https://github.com/dfinity/icp-cli-network-launcher/releases/download/{pkg_version}/icp-cli-network-launcher-{arch}-{os}-{pkg_version}.tar.gz"
    );
    let stream = client
        .get(&url)
        .send()
        .await
        .context(DownloadSnafu)?
        .error_for_status()
        .context(DownloadSnafu)?
        .bytes_stream();
    let mut tmp_write = stream
        .map(|res| res.context(DownloadSnafu))
        .try_fold(tmp_write, |mut tmp_write, chunk| async move {
            tmp_write.write_all(&chunk).context(SaveDownloadSnafu)?;
            Ok(tmp_write)
        })
        .await?;
    tmp_write.flush().context(SaveDownloadSnafu)?;
    drop(tmp_write);
    tmp.seek(SeekFrom::Start(0)).context(BufferSnafu)?;
    let tmp_read = std::io::BufReader::new(&tmp);
    let decompressor = GzDecoder::new(tmp_read);
    let mut archive = Archive::new(decompressor);
    let extract_dir = paths.launcher_dir().join("tmp");
    crate::fs::create_dir_all(&extract_dir).context(TempDirSnafu)?;
    let tarball_name = format!("icp-cli-network-launcher-{arch}-{os}-{pkg_version}");
    let extracted_dir_path = extract_dir.join(&tarball_name);
    if extracted_dir_path.exists() {
        crate::fs::remove_dir_all(&extracted_dir_path).context(RemoveExistingSnafu)?
    }
    archive
        .unpack(&extract_dir)
        .context(ExtractSnafu { path: &extract_dir })?;
    if version_path.exists() {
        crate::fs::remove_dir_all(&version_path).context(RemoveExistingSnafu)?
    }
    std::fs::rename(&extracted_dir_path, &version_path).context(MoveExtractedSnafu {
        from: extracted_dir_path,
        to: &version_path,
    })?;
    Ok((pkg_version, version_path.join(LAUNCHER_NAME)))
}

const ONE_DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Check whether a newer network launcher version is available on GitHub.
/// Returns `Some(latest_version)` if an update is available, `None` otherwise.
/// Throttled to at most once per day via a timestamp file in the package cache.
pub async fn check_launcher_update_available(
    paths: LWrite<&PackageCachePaths>,
    cached_version: &str,
    client: &Client,
) -> Option<String> {
    let ts_path = paths.update_nag_timestamp();
    if let Ok(contents) = crate::fs::read_to_string(&ts_path)
        && let Ok(ts) = contents.trim().parse::<u64>()
    {
        let then = SystemTime::UNIX_EPOCH + Duration::from_secs(ts);
        if then.elapsed().unwrap_or(Duration::ZERO) < ONE_DAY {
            debug!("Skipping launcher update check (last check < 24h ago)");
            return None;
        }
    }

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("since epoch")
        .as_secs();
    // Write timestamp regardless of outcome, so we don't re-check on failure
    let _ = crate::fs::write(&ts_path, format!("{now}\n").as_bytes());

    let latest = get_latest_launcher_version(client).await.ok()?;
    if latest != cached_version {
        Some(latest)
    } else {
        None
    }
}

#[derive(Debug, Snafu)]
pub enum DownloadLauncherError {
    #[snafu(display(
        "Unsupported network launcher architecture: {arch}. Supported architectures are: {supported:?}",
    ))]
    UnsupportedArch {
        arch: String,
        supported: Vec<&'static str>,
    },
    #[snafu(display(
        "Unsupported network launcher operating system: {os}. Supported operating systems are: {supported:?}",
    ))]
    UnsupportedOs {
        os: String,
        supported: Vec<&'static str>,
    },
    #[snafu(display("failed to download network launcher"))]
    Download { source: reqwest::Error },
    #[snafu(display("failed to save downloaded network launcher"))]
    SaveDownload { source: std::io::Error },
    #[snafu(display("failed to remove existing launcher"))]
    RemoveExisting { source: crate::fs::IoError },
    #[snafu(display("failed to create temporary file for download"))]
    TempFile { source: std::io::Error },
    #[snafu(display("failed to create temporary directory for extraction"))]
    TempDir { source: crate::fs::IoError },
    #[snafu(display("buffer failure in temporary file"))]
    Buffer { source: std::io::Error },
    #[snafu(display("failed to extract downloaded network launcher to {path}"))]
    Extract {
        source: std::io::Error,
        path: PathBuf,
    },
    #[snafu(display("failed to move extracted launcher from {from} to {to}"))]
    MoveExtracted {
        source: std::io::Error,
        from: PathBuf,
        to: PathBuf,
    },
    #[snafu(display("failed to create network launcher cache directory"))]
    CreateDir { source: crate::fs::IoError },
    #[snafu(display("failed to fetch latest network launcher version from GitHub"))]
    LatestVersionFetch { source: reqwest::Error },
    #[snafu(display("failed to parse latest version response from GitHub"))]
    LatestVersionParse,
    #[snafu(display("failed to create package tag"))]
    CreateTag { source: crate::fs::json::Error },
}
