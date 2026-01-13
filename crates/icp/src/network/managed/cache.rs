use std::io::{BufWriter, Seek, SeekFrom, Write};

use flate2::bufread::GzDecoder;
use futures::{StreamExt, TryStreamExt};
use reqwest::Client;
use snafu::{ResultExt, Snafu};
use tar::Archive;

use crate::fs::lock::PathsAccess;
use crate::fs::lock::{DirectoryStructureLock, LRead, LWrite, LockError};
use crate::prelude::*;

pub struct NetworkVersionCachePaths {
    root: PathBuf,
}

impl NetworkVersionCachePaths {
    pub fn launcher_dir(&self) -> PathBuf {
        self.root.join("network-launcher")
    }
    pub fn launcher_version(&self, version: &str) -> PathBuf {
        self.launcher_dir().join(version)
    }
}

pub type NetworkVersionCache = DirectoryStructureLock<NetworkVersionCachePaths>;

impl NetworkVersionCache {
    pub fn new(root: PathBuf) -> Result<Self, LockError> {
        DirectoryStructureLock::open_or_create(NetworkVersionCachePaths { root })
    }
}

impl PathsAccess for NetworkVersionCachePaths {
    fn lock_file(&self) -> PathBuf {
        self.root.join(".lock")
    }
}

pub fn get_cached_launcher_version(
    paths: LRead<NetworkVersionCachePaths>,
    version: &str,
) -> Option<PathBuf> {
    let version_path = paths.launcher_version(version);
    if version_path.exists() {
        Some(version_path.join("icp-cli-network-launcher"))
    } else {
        None
    }
}

pub async fn download_launcher_version(
    paths: LWrite<NetworkVersionCachePaths>,
    version: &str,
    client: &Client,
) -> Result<PathBuf, DownloadLauncherError> {
    let version_path = paths.launcher_version(version);
    crate::fs::create_dir_all(&version_path).context(CreateDirSnafu)?;
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
        "https://github.com/dfinity/icp-cli-network-launcher/releases/download/v{version}/icp-cli-network-launcher-{arch}-{os}-v{version}.tar.gz"
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
    if let Err(e) = archive.unpack(&version_path) {
        _ = std::fs::remove_dir_all(&version_path);
        return Err(e).context(ExtractSnafu);
    }
    Ok(version_path.join("icp-cli-network-launcher"))
}

#[derive(Debug, Snafu)]
pub enum DownloadLauncherError {
    #[snafu(display(
        "Unsupported network launcher architecture: {}. Supported architectures are: {:?}",
        arch,
        supported
    ))]
    UnsupportedArch {
        arch: String,
        supported: Vec<&'static str>,
    },
    #[snafu(display(
        "Unsupported network launcher operating system: {}. Supported operating systems are: {:?}",
        os,
        supported
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
    #[snafu(display("buffer failure in temporary file"))]
    Buffer { source: std::io::Error },
    #[snafu(display("failed to extract downloaded network launcher"))]
    Extract { source: std::io::Error },
    #[snafu(display("failed to create network launcher cache directory"))]
    CreateDir { source: crate::fs::IoError },
}
