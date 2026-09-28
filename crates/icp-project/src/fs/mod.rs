use snafu::prelude::*;
use std::io::{self, ErrorKind};

use crate::prelude::*;

pub mod json;
pub mod lock;
pub mod yaml;

#[derive(Debug, Snafu)]
#[snafu(display("Filesystem operation failed at {path}"))]
pub struct IoError {
    source: io::Error,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
#[snafu(display("Failed to rename `{from}` to `{to}`"))]
pub struct RenameError {
    source: io::Error,
    from: PathBuf,
    to: PathBuf,
}

#[derive(Debug, Snafu)]
#[snafu(display("Failed to copy `{from}` to `{to}`"))]
pub struct CopyError {
    source: io::Error,
    from: PathBuf,
    to: PathBuf,
}

impl IoError {
    pub fn kind(&self) -> ErrorKind {
        self.source.kind()
    }
}

pub fn create_dir_all(path: &Path) -> Result<(), IoError> {
    std::fs::create_dir_all(path).context(IoSnafu { path })
}

pub fn read(path: &Path) -> Result<Vec<u8>, IoError> {
    std::fs::read(path).context(IoSnafu { path })
}

pub fn read_to_string(path: &Path) -> Result<String, IoError> {
    std::fs::read_to_string(path).context(IoSnafu { path })
}

/// Non-recursive directory listing, as full paths.
/// A name that is not UTF-8 is skipped rather than being an error.
pub fn read_dir(path: &Path) -> Result<Vec<PathBuf>, IoError> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path).context(IoSnafu { path })? {
        let entry = entry.context(IoSnafu { path })?;
        if let Ok(p) = PathBuf::from_path_buf(entry.path()) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

pub fn remove_dir_all(path: &Path) -> Result<(), IoError> {
    std::fs::remove_dir_all(path).context(IoSnafu { path })
}

pub fn remove_file(path: &Path) -> Result<(), IoError> {
    std::fs::remove_file(path).context(IoSnafu { path })
}

pub fn copy(from: &Path, to: &Path) -> Result<(), CopyError> {
    std::fs::copy(from, to)
        .map(|_| ())
        .context(CopySnafu { from, to })
}

pub fn rename(from: &Path, to: &Path) -> Result<(), RenameError> {
    std::fs::rename(from, to).context(RenameSnafu { from, to })
}

pub fn write(path: &Path, contents: &[u8]) -> Result<(), IoError> {
    std::fs::write(path, contents).context(IoSnafu { path })
}

pub fn write_string(path: &Path, contents: &str) -> Result<(), IoError> {
    std::fs::write(path, contents.as_bytes()).context(IoSnafu { path })
}

/// Writes `contents` to a file only its owner can read or write.
///
/// On Unix the file is created `0600`, and a file that already exists is
/// narrowed to `0600` before any of `contents` reaches it. Elsewhere it is a
/// plain [`write`], leaving access to the directory's inherited ACL.
pub fn write_private(path: &Path, contents: &[u8]) -> Result<(), IoError> {
    #[cfg(unix)]
    {
        use std::{
            fs::{OpenOptions, Permissions},
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .context(IoSnafu { path })?;
        file.set_permissions(Permissions::from_mode(0o600))
            .context(IoSnafu { path })?;
        file.write_all(contents).context(IoSnafu { path })
    }
    #[cfg(not(unix))]
    {
        write(path, contents)
    }
}

/// Creates `path` and its missing parents, then makes `path` itself a directory
/// only its owner can enter (`0700` on Unix; elsewhere a plain [`create_dir_all`]).
///
/// The mode is applied even when `path` already exists, so a directory created
/// earlier with looser permissions is tightened.
pub fn create_private_dir_all(path: &Path) -> Result<(), IoError> {
    create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};
        std::fs::set_permissions(path, Permissions::from_mode(0o700)).context(IoSnafu { path })?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use camino_tempfile::Utf8TempDir;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn write_private_creates_an_owner_only_file() {
        let tmp = Utf8TempDir::new().unwrap();
        let path = tmp.path().join("key.pem");
        write_private(&path, b"secret").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
    }

    #[test]
    fn write_private_narrows_an_existing_file() {
        let tmp = Utf8TempDir::new().unwrap();
        let path = tmp.path().join("seed.txt");
        std::fs::write(&path, "a much longer old body").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"new").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn create_private_dir_all_tightens_an_existing_directory() {
        let tmp = Utf8TempDir::new().unwrap();
        let dir = tmp.path().join("identity/keys");
        create_private_dir_all(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir_all(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
    }
}
