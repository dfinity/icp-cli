use camino::{Utf8Path, Utf8PathBuf};
use fd_lock::RwLockWriteGuard;
use std::fs::File;

/// Encapsulates our claim on a lock file.
/// In this context, a lock file is a 0-length file used to manage
/// exclusive access to a resource, such as a network port.
/// Deletes the lock and releases the lock when dropped.
pub struct LockFileClaim<'a> {
    path: Utf8PathBuf,
    _guard: RwLockWriteGuard<'a, File>,
}

impl<'a> LockFileClaim<'a> {
    pub fn new(path: impl AsRef<Utf8Path>, guard: RwLockWriteGuard<'a, File>) -> Self {
        let path = path.as_ref().to_path_buf();
        Self {
            path,
            _guard: guard,
        }
    }
}

impl Drop for LockFileClaim<'_> {
    fn drop(&mut self) {
        // On Windows, the file can't be removed while it's locked,
        // so we just leave it in place in order to avoid potential
        // race conditions.
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}
