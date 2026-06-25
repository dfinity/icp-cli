//! Path-safety helpers shared between the host runtime (`dirs` preopens) and
//! the CLI's plugin sync (`files` reads).

use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

/// Returns `true` if `rel` cannot be safely joined onto a base directory
/// because it contains a component that would escape it: `..`, a filesystem
/// root, or a (Windows) drive prefix such as `C:` — the latter makes a path
/// drive-relative even without a leading separator, so `is_absolute()` returns
/// `false` yet joining it discards the base. Mirrors the escape checks in the
/// bundler (`crates/icp-cli/src/operations/bundle.rs`).
///
/// Callers reject such paths before resolving them; `first_symlink_component`
/// only inspects `Normal` components and so would not otherwise catch these.
pub(crate) fn escapes_base(rel: &str) -> bool {
    Utf8Path::new(rel).components().any(|c| {
        matches!(
            c,
            Utf8Component::ParentDir | Utf8Component::RootDir | Utf8Component::Prefix(_)
        )
    })
}

/// Walks `rel` one component at a time under `base` and returns the first
/// sub-path of `rel` (relative to `base`) that is a symlink, if any.
///
/// Declared `dirs`/`files` entries are resolved on the host *before* the WASI
/// sandbox boundary, so a symlinked entry — or an entry that traverses a
/// symlinked directory — would let a preopen or a host read escape `base` to an
/// arbitrary location on disk (the lexical [`escapes_base`] check does not catch
/// this). Rejecting any symlink in the declared portion keeps every preopen and
/// read anchored within `base`. Symlinks *inside* a preopen that escape it are
/// separately rejected by the WASI sandbox (cap-std) at runtime.
///
/// The returned path is relative to `base` (e.g. `link` or `link/inner`),
/// matching what the user wrote in the manifest, so it can be surfaced in an
/// error without leaking the absolute on-disk location.
///
/// `base` itself may be reached through symlinks (e.g. the project lives under
/// a symlinked path); only the declared relative portion is checked.
///
/// `rel` is expected to be relative and free of `..` (callers validate that via
/// [`escapes_base`] first); `.` components are ignored. Components that do not
/// exist are not symlinks, so a missing path returns `None` and the subsequent
/// read or preopen surfaces the not-found error.
pub(crate) fn first_symlink_component(base: &Utf8Path, rel: &str) -> Option<Utf8PathBuf> {
    let mut host = base.to_path_buf();
    let mut relative = Utf8PathBuf::new();
    for component in Utf8Path::new(rel).components() {
        if let Utf8Component::Normal(name) = component {
            host.push(name);
            relative.push(name);
            match std::fs::symlink_metadata(host.as_std_path()) {
                Ok(meta) if meta.file_type().is_symlink() => return Some(relative),
                _ => {}
            }
        }
    }
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    use camino_tempfile::tempdir;

    #[test]
    fn escapes_base_flags_unsafe_components() {
        assert!(!escapes_base("a/b"));
        assert!(!escapes_base("./a"));
        assert!(escapes_base("../a"));
        assert!(escapes_base("a/../b"));
        // Absolute paths carry a `RootDir` component on this (Unix) host. The
        // Windows drive-prefix case (`C:foo`) only parses as `Prefix` on
        // Windows and so cannot be exercised here.
        assert!(escapes_base("/abs"));
    }

    #[test]
    fn plain_relative_path_has_no_symlink() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        std::fs::write(base.join("a/b/file.txt"), b"hi").unwrap();

        assert_eq!(first_symlink_component(base, "a/b"), None);
        assert_eq!(first_symlink_component(base, "a/b/file.txt"), None);
    }

    #[test]
    fn final_entry_is_symlink() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();
        std::fs::create_dir_all(base.join("real")).unwrap();
        symlink(base.join("real"), base.join("link")).unwrap();

        assert_eq!(
            first_symlink_component(base, "link"),
            Some(Utf8PathBuf::from("link"))
        );
    }

    #[test]
    fn intermediate_component_is_symlink() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();
        // base/real/inner exists; base/link -> base/real, so "link/inner"
        // traverses a symlink even though "inner" itself is a real dir.
        std::fs::create_dir_all(base.join("real/inner")).unwrap();
        symlink(base.join("real"), base.join("link")).unwrap();

        // The reported path is the offending sub-path relative to `base`,
        // i.e. the symlinked component, not the trailing real directory.
        assert_eq!(
            first_symlink_component(base, "link/inner"),
            Some(Utf8PathBuf::from("link"))
        );
    }

    #[test]
    fn missing_path_is_not_a_symlink() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();
        assert_eq!(first_symlink_component(base, "does/not/exist"), None);
    }

    #[test]
    fn dot_components_are_ignored() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();
        std::fs::create_dir_all(base.join("a")).unwrap();
        assert_eq!(first_symlink_component(base, "./a"), None);
    }

    #[test]
    fn symlinked_base_is_allowed() {
        // A symlink *above* the declared portion (i.e. reaching `base`) is fine;
        // only components of `rel` are checked.
        let tmp = tempdir().unwrap();
        let real_base = tmp.path().join("real-base");
        std::fs::create_dir_all(real_base.join("data")).unwrap();
        let linked_base = tmp.path().join("linked-base");
        symlink(&real_base, &linked_base).unwrap();

        assert_eq!(first_symlink_component(&linked_base, "data"), None);
    }
}
