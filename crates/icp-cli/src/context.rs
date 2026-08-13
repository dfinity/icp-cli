//! The CLI's execution context.
//!
//! Wraps the library [`icp::context::Context`] — which is a bag of ports for
//! building and deploying — with the frontend-only state the library has no
//! business knowing about, such as presentation flags. Derefs to the library
//! context, so every library port (`dirs`, `ids`, `project`, `network`, …) is
//! reached straight through it.

use std::{env::current_dir, ops::Deref, sync::Arc, time::Duration};

use icp::{
    canister::recipe::handlebars::Handlebars,
    directories::{Access as _, Directories},
    identity::PasswordFunc,
    prelude::*,
};
use snafu::prelude::*;

use crate::{
    manifest::ProjectRootLocateImpl,
    project::{Lazy, ProjectLoadImpl},
};

/// Execution context for a single CLI invocation.
#[derive(Clone)]
pub struct Context {
    /// The library context.
    inner: icp::context::Context,

    /// Whether debug output is enabled (`--debug`). Presentation only: it
    /// selects the tracing layer and hides progress bars.
    pub debug: bool,
}

impl Deref for Context {
    type Target = icp::context::Context;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug, Snafu)]
pub enum ContextInitError {
    #[snafu(display("failed to initialize directories"))]
    Directories {
        source: icp::directories::DirectoriesError,
    },

    #[snafu(display("failed to get current working directory"))]
    Cwd { source: std::io::Error },

    #[snafu(display("failed to convert path to UTF-8"))]
    Utf8Path { source: FromPathBufError },

    #[snafu(display("failed to lock package cache directory"))]
    PackageCache { source: icp::fs::lock::LockError },

    #[snafu(transparent)]
    Library {
        source: icp::context::ContextInitError,
    },
}

/// Builds the context for this CLI invocation.
pub fn initialize(
    project_root_override: Option<PathBuf>,
    debug: bool,
    password_func: PasswordFunc,
    pem_session_duration: Option<Duration>,
) -> Result<Context, ContextInitError> {
    // Setup global directory structure
    let dirs = Arc::new(Directories::new().context(DirectoriesSnafu)?);

    // Project root locator
    let project_root_locate = Arc::new(ProjectRootLocateImpl::new(
        resolve_cwd()?,
        project_root_override,
    ));

    // Recipes
    let recipe = Arc::new(Handlebars {
        http_client: reqwest::Client::new(),
        pkg_cache: dirs.package_cache().context(PackageCacheSnafu)?,
    });

    // Project loader
    let project = Arc::new(Lazy::new(ProjectLoadImpl {
        project_root_locate: project_root_locate.clone(),
        recipe,
    }));

    let inner = icp::context::initialize(
        dirs,
        project_root_locate,
        project,
        password_func,
        pem_session_duration,
    )?;

    Ok(Context { inner, debug })
}

/// The directory to start looking for a project in.
///
/// On Unix, prefer $PWD (the logical path the user cd'd through) over
/// getcwd(3), which resolves symlinks to the physical path and would break
/// upward traversal when the user is inside a symlinked directory whose
/// manifest sits above the symlink's location.
///
/// Guard with an inode check: if $PWD was inherited from a parent process that
/// used chdir(2) without updating $PWD, the two paths point to different inodes
/// and we fall back to getcwd(). Because `metadata()` follows symlinks, a
/// symlinked $PWD still resolves to the same inode as getcwd(), so the symlink
/// case still works.
#[cfg(unix)]
fn resolve_cwd() -> Result<PathBuf, ContextInitError> {
    let real = PathBuf::try_from(current_dir().context(CwdSnafu)?).context(Utf8PathSnafu)?;
    Ok(std::env::var("PWD")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .filter(|p| same_inode(p.as_path(), real.as_path()))
        .unwrap_or(real))
}

#[cfg(not(unix))]
fn resolve_cwd() -> Result<PathBuf, ContextInitError> {
    PathBuf::try_from(current_dir().context(CwdSnafu)?).context(Utf8PathSnafu)
}

#[cfg(unix)]
fn same_inode(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::sync::Mutex;

    use camino_tempfile::Utf8TempDir;

    use super::*;

    // Serializes tests that mutate $PWD, since cargo test runs tests in parallel.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn stale_pwd_is_ignored() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let stale = Utf8TempDir::new().unwrap();
        let real = PathBuf::try_from(std::env::current_dir().unwrap()).unwrap();

        let old_pwd = std::env::var("PWD").ok();
        // SAFETY: ENV_MUTEX serializes all tests that mutate $PWD.
        unsafe { std::env::set_var("PWD", stale.path()) };

        let resolved = resolve_cwd().unwrap();

        match old_pwd {
            Some(v) => unsafe { std::env::set_var("PWD", v) },
            None => unsafe { std::env::remove_var("PWD") },
        }

        assert_eq!(
            resolved, real,
            "stale $PWD should be ignored in favour of getcwd()"
        );
    }
}
