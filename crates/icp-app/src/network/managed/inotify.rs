//! Why Linux refused a native file watcher, when the reason is a resource
//! limit rather than anything about the path.
//!
//! On Linux the native watcher is an inotify instance, and the kernel rations
//! those three ways. `fs.inotify.max_user_instances` caps the instances one
//! user may hold across every process, and `RLIMIT_NOFILE` caps the
//! descriptors one process may hold; `inotify_init` reports either as
//! `EMFILE`, so the errno alone cannot say which. `fs.inotify.max_user_watches`
//! caps the paths one user may watch, and `inotify_add_watch` reports that as
//! `ENOSPC`, which `notify` turns into its own `MaxFilesWatch` kind.
//!
//! Desktop editors and file-sync clients hold instances for as long as they
//! run, so a developer machine can be at the instance quota with nothing
//! wrong on our side. Nothing here changes a limit: it reads the ones involved
//! so the warning can name them and say what would raise them.

use std::fmt;

/// A resource limit that stopped a native watcher from being created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherLimit {
    /// `inotify_init` failed with `EMFILE`: the user's inotify instance quota,
    /// or this process's open-file limit.
    Instances(Limits),
    /// `inotify_add_watch` failed with `ENOSPC`: the user's inotify watch quota.
    Watches(Limits),
}

/// What the host reports about the limits involved. Each is `None` when it
/// could not be read, which is never a reason not to warn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limits {
    /// `fs.inotify.max_user_instances`
    pub max_user_instances: Option<u64>,
    /// `fs.inotify.max_user_watches`
    pub max_user_watches: Option<u64>,
    /// This process's soft `RLIMIT_NOFILE`.
    pub open_files_limit: Option<u64>,
    /// The descriptors this process holds right now.
    pub open_files: Option<u64>,
}

impl WatcherLimit {
    /// The limit `err` reports, with the host's figures for it; `None` when
    /// `err` is about something else, or on a platform without inotify.
    pub fn of(err: &notify::Error) -> Option<Self> {
        Self::with_limits(err, Limits::read())
    }

    /// [`WatcherLimit::of`] with the figures supplied, for a message about a
    /// host other than this one.
    pub fn with_limits(err: &notify::Error, limits: Limits) -> Option<Self> {
        match cause(err)? {
            Cause::Instances => Some(Self::Instances(limits)),
            Cause::Watches => Some(Self::Watches(limits)),
        }
    }
}

enum Cause {
    Instances,
    Watches,
}

#[cfg(target_os = "linux")]
fn cause(err: &notify::Error) -> Option<Cause> {
    use notify::ErrorKind;
    match &err.kind {
        ErrorKind::MaxFilesWatch => Some(Cause::Watches),
        ErrorKind::Io(io) if io.raw_os_error() == Some(nix::errno::Errno::EMFILE as i32) => {
            Some(Cause::Instances)
        }
        _ => None,
    }
}

#[cfg(not(target_os = "linux"))]
fn cause(_err: &notify::Error) -> Option<Cause> {
    None
}

impl Limits {
    /// The host's figures, each read on its own so one that cannot be read
    /// costs nothing but itself.
    #[cfg(target_os = "linux")]
    pub fn read() -> Self {
        Self {
            max_user_instances: read_number("/proc/sys/fs/inotify/max_user_instances"),
            max_user_watches: read_number("/proc/sys/fs/inotify/max_user_watches"),
            open_files_limit: read_open_files_limit(),
            open_files: std::fs::read_dir("/proc/self/fd")
                .ok()
                .map(|entries| entries.count() as u64),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn read() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "linux")]
fn read_number(path: &str) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The soft `RLIMIT_NOFILE`, from the `Max open files` row of `/proc/self/limits`,
/// whose columns are the limit's name, its soft value, its hard value and a unit.
#[cfg(target_os = "linux")]
fn read_open_files_limit() -> Option<u64> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
    let row = limits.lines().find(|l| l.starts_with("Max open files"))?;
    let soft = row["Max open files".len()..].split_whitespace().next()?;
    soft.parse().ok()
}

/// How a figure reads in the message when the host did not give it.
fn figure(n: Option<u64>) -> String {
    n.map_or_else(|| "unknown".to_owned(), |n| n.to_string())
}

impl fmt::Display for WatcherLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instances(limits) => {
                write!(
                    f,
                    "Linux refused to create an inotify instance (EMFILE), which means either \
                     the per-user inotify instance quota is exhausted (fs.inotify.max_user_instances = {instances}; \
                     editors and file-sync clients hold instances for as long as they run) \
                     or this process reached its open-file limit",
                    instances = figure(limits.max_user_instances),
                )?;
                match (limits.open_files, limits.open_files_limit) {
                    (Some(open), Some(limit)) => write!(
                        f,
                        " ({open} of {limit} descriptors in use, so {verdict})",
                        verdict = if open + 8 >= limit {
                            "the open-file limit is the likely cause"
                        } else {
                            "the instance quota is the likely cause"
                        }
                    )?,
                    (open, limit) => write!(
                        f,
                        " ({} of {} descriptors in use)",
                        figure(open),
                        figure(limit)
                    )?,
                }
                write!(
                    f,
                    ". Raise the quota with `sudo sysctl fs.inotify.max_user_instances=1024` \
                     (persist it in a file under /etc/sysctl.d/), or close programs holding watchers."
                )
            }
            Self::Watches(limits) => write!(
                f,
                "Linux refused to add an inotify watch (ENOSPC): the per-user inotify watch quota is \
                 exhausted (fs.inotify.max_user_watches = {watches}). Raise it with \
                 `sudo sysctl fs.inotify.max_user_watches=524288` (persist it in a file under \
                 /etc/sysctl.d/), or close programs holding watches.",
                watches = figure(limits.max_user_watches),
            ),
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use notify::ErrorKind;
    use std::io;

    fn emfile() -> notify::Error {
        notify::Error::io(io::Error::from_raw_os_error(
            nix::errno::Errno::EMFILE as i32,
        ))
    }

    fn figures() -> Limits {
        Limits {
            max_user_instances: Some(128),
            max_user_watches: Some(8192),
            open_files_limit: Some(1048576),
            open_files: Some(41),
        }
    }

    #[test]
    fn emfile_at_init_is_the_instance_quota_or_the_descriptor_limit() {
        assert_eq!(
            WatcherLimit::with_limits(&emfile(), figures()),
            Some(WatcherLimit::Instances(figures()))
        );
    }

    #[test]
    fn enospc_at_add_watch_is_the_watch_quota() {
        assert_eq!(
            WatcherLimit::with_limits(&notify::Error::new(ErrorKind::MaxFilesWatch), figures()),
            Some(WatcherLimit::Watches(figures()))
        );
    }

    #[test]
    fn any_other_refusal_is_not_a_limit() {
        let denied = notify::Error::io(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(WatcherLimit::with_limits(&denied, figures()), None);
        assert_eq!(
            WatcherLimit::with_limits(&notify::Error::path_not_found(), figures()),
            None
        );
    }

    #[test]
    fn the_instance_message_names_both_causes_and_the_remedy() {
        let message = WatcherLimit::Instances(figures()).to_string();
        assert!(message.contains("EMFILE"), "{message}");
        assert!(
            message.contains("fs.inotify.max_user_instances = 128"),
            "{message}"
        );
        assert!(message.contains("open-file limit"), "{message}");
        assert!(message.contains("41 of 1048576 descriptors"), "{message}");
        assert!(
            message.contains("the instance quota is the likely cause"),
            "{message}"
        );
        assert!(
            message.contains("sudo sysctl fs.inotify.max_user_instances=1024"),
            "{message}"
        );
    }

    #[test]
    fn a_process_at_its_descriptor_limit_is_told_so() {
        let limits = Limits {
            open_files_limit: Some(1024),
            open_files: Some(1023),
            ..figures()
        };
        let message = WatcherLimit::Instances(limits).to_string();
        assert!(
            message.contains("the open-file limit is the likely cause"),
            "{message}"
        );
    }

    #[test]
    fn a_figure_the_host_did_not_give_reads_as_unknown() {
        let message = WatcherLimit::Instances(Limits::default()).to_string();
        assert!(
            message.contains("fs.inotify.max_user_instances = unknown"),
            "{message}"
        );
        assert!(
            message.contains("unknown of unknown descriptors"),
            "{message}"
        );
    }

    #[test]
    fn the_watch_message_names_the_watch_quota_and_the_remedy() {
        let message = WatcherLimit::Watches(figures()).to_string();
        assert!(message.contains("ENOSPC"), "{message}");
        assert!(
            message.contains("fs.inotify.max_user_watches = 8192"),
            "{message}"
        );
        assert!(
            message.contains("sudo sysctl fs.inotify.max_user_watches=524288"),
            "{message}"
        );
    }

    #[test]
    fn this_host_reports_its_limits() {
        let limits = Limits::read();
        assert!(limits.max_user_instances.is_some_and(|n| n > 0));
        assert!(limits.max_user_watches.is_some_and(|n| n > 0));
        assert!(limits.open_files_limit.is_some_and(|n| n > 0));
        // stdin, stdout and stderr at the least
        assert!(limits.open_files.is_some_and(|n| n >= 3));
    }
}
