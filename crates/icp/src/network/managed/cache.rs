use std::io::{BufWriter, Seek, SeekFrom, Write};

use flate2::bufread::GzDecoder;
use futures::{StreamExt, TryStreamExt};
use reqwest::Client;
use snafu::{ResultExt, Snafu};
use tar::Archive;

use crate::fs::lock::{LRead, LWrite};
use crate::package::{PackageCachePaths, get_tag, set_tag};
use crate::prelude::*;

pub fn get_cached_launcher_version(
    paths: LRead<&PackageCachePaths>,
    version: &str,
) -> Result<Option<PathBuf>, ReadCacheError> {
    let declared_version = if version == "latest" {
        let Some(version) =
            get_tag(paths, "icp-cli-network-launcher", "latest").context(LoadTagSnafu)?
        else {
            return Ok(None);
        };
        version.to_owned()
    } else {
        format!("v{version}")
    };
    let version_path = paths.launcher_version(&declared_version);
    if version_path.exists() {
        Ok(Some(version_path.join("icp-cli-network-launcher")))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Snafu)]
pub enum ReadCacheError {
    #[snafu(display("failed to read package tag"))]
    LoadTag { source: crate::fs::json::Error },
}

pub async fn get_latest_launcher_version(client: &Client) -> Result<String, DownloadLauncherError> {
    let url = "https://api.github.com/repos/dfinity/icp-cli-network-launcher/releases/latest";
    let response: serde_json::Value = client
        .get(url)
        .header("User-Agent", "icp-cli")
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
) -> Result<PathBuf, DownloadLauncherError> {
    let pkg_version = if version_req == "latest" {
        let latest = get_latest_launcher_version(client).await?;
        set_tag(paths, "icp-cli-network-launcher", &latest, "latest").context(CreateTagSnafu)?;
        latest
    } else {
        format!("v{version_req}")
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
    let extract_dir = camino_tempfile::tempdir().context(TempDirSnafu)?;
    archive.unpack(extract_dir.path()).context(ExtractSnafu {
        path: extract_dir.path(),
    })?;
    let tarball_name = format!("icp-cli-network-launcher-{arch}-{os}-{pkg_version}");
    let extracted_inner = extract_dir.path().join(&tarball_name);
    std::fs::rename(&extracted_inner, &version_path).context(MoveExtractedSnafu {
        from: extracted_inner,
        to: &version_path,
    })?;
    Ok(version_path.join("icp-cli-network-launcher"))
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
    #[snafu(display("failed to create temporary file for download"))]
    TempFile { source: std::io::Error },
    #[snafu(display("failed to create temporary directory for extraction"))]
    TempDir { source: std::io::Error },
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
