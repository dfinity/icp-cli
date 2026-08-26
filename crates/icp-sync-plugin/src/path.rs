//! Path-safety helpers used by the host runtime to validate declared `dirs`/`files`
//! entries before preopening directories or reading files inside the project.

use std::collections::HashSet;

use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

/// Why a declared `dirs`/`files` entry cannot be anchored inside the sandbox
/// root. Reported by [`resolve`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Escape {
    /// The entry is absolute, or (on Windows) drive-relative such as `C:foo` —
    /// which makes it drive-relative even without a leading separator, so
    /// `is_absolute()` returns `false` yet joining it discards the base.
    NotRelative,
    /// The entry rose above the sandbox root with `..`.
    AboveRoot,
}

/// A declared `dirs`/`files` entry resolved against the sandbox root.
///
/// Produced by [`resolve`]; the components are free of `.` and `..`, so the
/// on-disk location is [`Self::path`] joined onto the root — never the declared
/// path joined onto the base directory, which for an entry containing `..`
/// would be resolved by the OS through whatever the base directory's own
/// components happen to be.
#[derive(Debug)]
pub(crate) struct Resolved<'a> {
    /// The entry's location relative to the sandbox root.
    components: Vec<&'a str>,
    /// Index of the first component the symlink check covers. For an entry
    /// that stays below the base directory this is the whole base-directory
    /// ancestry, exempt for the same reason the root itself is: how the project
    /// reaches its own canister directory is not something a manifest declared.
    /// An entry that rises out of the base directory is checked from the root
    /// (index 0) — it re-anchors at an ancestor and descends somewhere the base
    /// directory's own path never went, so none of that ancestry can be assumed
    /// to lead where it lexically says it does.
    traversed_from: usize,
}

/// The base directory's position relative to the sandbox root, as the
/// components of a `.`/`..`-free relative path.
///
/// `None` when `base` lies outside `root` — a dependency project reached by an
/// out-of-tree `path:` — which callers treat as "the base directory is its own
/// root", granting nothing above it.
pub(crate) fn base_within_root<'a>(root: &Utf8Path, base: &'a Utf8Path) -> Option<Vec<&'a str>> {
    let rel = base.strip_prefix(root).ok()?;
    let mut components = Vec::new();
    for component in rel.components() {
        match component {
            Utf8Component::Normal(name) => components.push(name),
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                components.pop()?;
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => return None,
        }
    }
    Some(components)
}

/// Resolve a declared entry, written relative to the base directory, against
/// the sandbox root.
///
/// `base` is the base directory's own position relative to the root (see
/// [`base_within_root`]). The entry may rise out of the base directory into the
/// rest of the project; it may not rise above the root, be absolute, or carry a
/// drive prefix. Mirrors the escape checks in the bundler
/// (`crates/icp-cli/src/operations/bundle.rs`).
pub(crate) fn resolve<'a>(base: &[&'a str], rel: &'a str) -> Result<Resolved<'a>, Escape> {
    let mut components = base.to_vec();
    let mut traversed_from = base.len();
    for component in Utf8Path::new(rel).components() {
        match component {
            Utf8Component::Normal(name) => components.push(name),
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                components.pop().ok_or(Escape::AboveRoot)?;
                // Rising out of the base directory re-anchors the entry on an
                // ancestor of it, so that ancestry stops being ambient: a
                // symlink anywhere in it would put the entry's target outside
                // the root even though the base directory itself is inside.
                if components.len() < base.len() {
                    traversed_from = 0;
                }
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => return Err(Escape::NotRelative),
        }
    }
    Ok(Resolved {
        components,
        traversed_from,
    })
}

impl Resolved<'_> {
    /// The entry's location relative to the sandbox root.
    pub(crate) fn path(&self) -> Utf8PathBuf {
        self.components.iter().copied().collect()
    }

    /// Walks the entry one component at a time under `root` and returns the
    /// first sub-path that is a symlink, if any.
    ///
    /// Declared `dirs`/`files` entries are resolved on the host *before* the
    /// WASI sandbox boundary, so a symlinked entry — or an entry that traverses
    /// a symlinked directory — would let a preopen or a host read escape the
    /// project to an arbitrary location on disk (the lexical [`resolve`] check
    /// does not catch this). Rejecting any symlink in the traversed portion
    /// keeps every preopen and read anchored within the project. Symlinks
    /// *inside* a preopen that escape it are separately rejected by the WASI
    /// sandbox (cap-std) at runtime.
    ///
    /// The returned path is relative to `root`, so it can be surfaced in an
    /// error without leaking the absolute on-disk location.
    ///
    /// `root` itself may be reached through symlinks (e.g. the project lives
    /// under a symlinked path), as may the base directory — but only for an
    /// entry that stays below it (see [`Resolved::traversed_from`]).
    /// Components that do not exist are not symlinks, so a
    /// missing path returns `None` and the subsequent read or preopen surfaces
    /// the not-found error.
    pub(crate) fn first_symlink_component(&self, root: &Utf8Path) -> Option<Utf8PathBuf> {
        let mut relative: Utf8PathBuf = self.components[..self.traversed_from]
            .iter()
            .copied()
            .collect();
        let mut host = root.join(&relative);
        for name in &self.components[self.traversed_from..] {
            host.push(name);
            relative.push(name);
            match std::fs::symlink_metadata(host.as_std_path()) {
                Ok(meta) if meta.file_type().is_symlink() => return Some(relative),
                _ => {}
            }
        }
        None
    }
}

/// The meaningful components of a declared relative path: the `/`-separated
/// names, with empty and `.` components dropped, so `./data/` and `data` compare
/// equal.
///
/// `\` is deliberately not a separator here. On Unix it is an ordinary character
/// in a filename, and these comparisons decide what gets opened for a guest that
/// will open the path exactly as written.
fn components(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

/// Reduce declared directories to the ones that actually have to be opened.
///
/// `dirs` is configuration as much as it is a sandbox grant: a plugin may
/// legitimately be handed the same tree under several keys, or a tree and a
/// subtree of it, and it is told about every entry that was declared. The grant
/// behind those entries has no such multiplicity — opening a directory twice, or
/// opening one already reachable through an ancestor, conveys no further access.
/// Callers keep the declared list as configuration and open only what this
/// returns; a nested declared directory is reached through the ancestor covering
/// it.
///
/// Retained paths keep their written spelling and first-occurrence order.
/// Comparison is over the written spelling rather than the resolved location,
/// because the guest opens each entry at the spelling the manifest gave it, and
/// is component-wise, so `data` covers `./data/inner` but not `database`.
///
/// A spelling prefix alone is not containment once entries may contain `..`:
/// `..` is a prefix of `../../shared`, yet one is the canister directory's
/// parent and the other a child of its grandparent — neither holds the other.
/// So an entry only covers one whose remaining components descend, `..`-free.
/// Two spellings that coincide only once resolved (`../data` and `data` from a
/// canister in `data`'s parent) still stay separate, which merely leaves the
/// result less reduced.
pub fn covering_dirs<'a>(dirs: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let dirs: Vec<&str> = dirs.into_iter().collect();
    let parts: Vec<Vec<&str>> = dirs.iter().map(|dir| components(dir)).collect();
    dirs.iter()
        .enumerate()
        .filter(|(i, _)| {
            !parts.iter().enumerate().any(|(j, other)| {
                j != *i
                    && parts[*i].starts_with(other)
                    && !parts[*i][other.len()..].contains(&"..")
                    // A strict ancestor always covers; between equals, the first written wins.
                    && (other.len() < parts[*i].len() || j < *i)
            })
        })
        .map(|(_, dir)| *dir)
        .collect()
}

/// Reduce declared paths to the distinct ones, keeping the written spelling and
/// first-occurrence order.
///
/// [`covering_dirs`] without the containment rule, for entries that name files:
/// `./a.json` and `a.json` are one file, but a file never subsumes another the
/// way a directory subsumes its contents.
pub fn distinct_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen: HashSet<Vec<&str>> = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(components(path)))
        .collect()
}

#[cfg(test)]
mod covering_tests {
    use super::*;

    #[test]
    fn unrelated_dirs_are_all_kept() {
        assert_eq!(
            covering_dirs(["assets", "config", "data/seed"]),
            ["assets", "config", "data/seed"],
        );
    }

    #[test]
    fn duplicates_collapse_to_the_first_spelling() {
        assert_eq!(covering_dirs(["./data", "data", "data/"]), ["./data"]);
    }

    #[test]
    fn nested_dirs_collapse_to_their_ancestor_whichever_is_written_first() {
        assert_eq!(covering_dirs(["data", "data/inner"]), ["data"]);
        assert_eq!(covering_dirs(["data/inner", "data"]), ["data"]);
        // Transitive: `data` covers `data/a` covers `data/a/b`.
        assert_eq!(covering_dirs(["data/a/b", "data/a", "data"]), ["data"]);
    }

    #[test]
    fn a_name_prefix_is_not_an_ancestor() {
        assert_eq!(covering_dirs(["data", "database"]), ["data", "database"]);
    }

    /// An entry reaching further out than another is not inside it, however
    /// much of its spelling they share: `..` is the canister directory's
    /// parent, `../../shared` a child of its grandparent. Collapsing them would
    /// leave the second with no preopen of its own and none that contains it.
    #[test]
    fn an_entry_that_rises_further_is_not_covered() {
        assert_eq!(
            covering_dirs(["..", "../../shared"]),
            ["..", "../../shared"],
        );
        assert_eq!(
            covering_dirs(["../shared", "../shared/../assets"]),
            ["../shared", "../shared/../assets"],
        );
    }

    /// Entries that reach out of the canister directory still cover what
    /// descends from them, and still collapse with a repeat of themselves.
    #[test]
    fn entries_outside_the_canister_dir_cover_their_own_contents() {
        assert_eq!(covering_dirs(["../data", "../data/inner"]), ["../data"]);
        assert_eq!(covering_dirs(["../data", "./../data"]), ["../data"]);
    }

    #[test]
    fn distinct_paths_dedupes_without_containment() {
        assert_eq!(
            distinct_paths(["./a.json", "a.json", "b.json", "dir/a.json"]),
            ["./a.json", "b.json", "dir/a.json"],
        );
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    /// The base directory a canister in `backend/` sits at, relative to the
    /// project root.
    const BASE: &[&str] = &["backend"];

    /// The resolved location of `rel`, as a `/`-joined project-relative path.
    fn at(base: &[&str], rel: &str) -> Result<String, Escape> {
        Ok(resolve(base, rel)?.path().to_string())
    }

    /// The base directory's position within the root, `/`-joined.
    fn base_rel(root: &str, base: &str) -> Option<String> {
        base_within_root(Utf8Path::new(root), Utf8Path::new(base)).map(|parts| parts.join("/"))
    }

    #[test]
    fn plain_relative_paths_resolve_under_the_base_dir() {
        assert_eq!(at(BASE, "a/b").unwrap(), "backend/a/b");
        assert_eq!(at(BASE, "./a").unwrap(), "backend/a");
        assert_eq!(at(BASE, "a/b/file.txt").unwrap(), "backend/a/b/file.txt");
        // A base at the project root leaves the declared path as written.
        assert_eq!(at(&[], "a/b").unwrap(), "a/b");
    }

    #[test]
    fn parent_components_reach_the_rest_of_the_project() {
        assert_eq!(at(BASE, "../shared").unwrap(), "shared");
        assert_eq!(at(BASE, "a/../b").unwrap(), "backend/b");
        assert_eq!(
            at(&["services", "crm"], "../../shared/seed").unwrap(),
            "shared/seed"
        );
        // Rising exactly to the root is fine; the root itself is in bounds.
        assert_eq!(at(BASE, "..").unwrap(), "");
    }

    #[test]
    fn rising_above_the_root_is_rejected() {
        assert_eq!(at(BASE, "../.."), Err(Escape::AboveRoot));
        assert_eq!(at(&[], "../a"), Err(Escape::AboveRoot));
        assert_eq!(at(BASE, "../../../elsewhere"), Err(Escape::AboveRoot));
    }

    #[test]
    fn absolute_paths_are_rejected() {
        // An absolute path carries a `RootDir` component on every platform.
        assert_eq!(at(BASE, "/abs"), Err(Escape::NotRelative));
    }

    // On Windows a drive-relative path like `C:foo` has a `Prefix` component
    // yet is NOT absolute, so an `is_absolute()` check alone would admit it and
    // joining it onto a base would discard the base. `resolve` must reject it.
    // (On Unix the same string is just an ordinary filename — see below.)
    #[cfg(windows)]
    #[test]
    fn windows_drive_and_unc_prefixes_are_rejected() {
        assert_eq!(at(BASE, "C:foo"), Err(Escape::NotRelative)); // drive-relative
        assert_eq!(at(BASE, r"C:\foo"), Err(Escape::NotRelative)); // absolute
        assert_eq!(at(BASE, r"\\server\share\x"), Err(Escape::NotRelative)); // UNC
    }

    #[cfg(unix)]
    #[test]
    fn unix_treats_drive_prefix_as_a_plain_name() {
        // There is no `Prefix` parsing on Unix, so `C:foo` is just a (weird)
        // filename with no escaping component.
        assert_eq!(at(BASE, "C:foo").unwrap(), "backend/C:foo");
    }

    #[test]
    fn base_within_root_is_the_path_from_the_root_down() {
        assert_eq!(
            base_rel("/work", "/work/backend").as_deref(),
            Some("backend")
        );
        assert_eq!(base_rel("/work", "/work").as_deref(), Some(""));
        assert_eq!(
            base_rel(".", "./services/crm").as_deref(),
            Some("services/crm")
        );
    }

    #[test]
    fn base_outside_the_root_has_no_position_within_it() {
        // A dependency reached by an out-of-tree `path:` keeps the root as a
        // prefix lexically, but resolving the `..` leaves the root behind.
        assert_eq!(base_rel("/work", "/work/../outside/backend"), None);
        // An unrelated directory is not under the root at all.
        assert_eq!(base_rel("/work", "/elsewhere/backend"), None);
    }
}

#[cfg(all(test, unix))]
mod symlink_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    use camino_tempfile::tempdir;

    /// The first symlinked component of `rel`, declared from a canister at
    /// `base` and resolved against `root`.
    fn first_symlink_from(root: &Utf8Path, base: &[&str], rel: &str) -> Option<Utf8PathBuf> {
        resolve(base, rel).unwrap().first_symlink_component(root)
    }

    /// [`first_symlink_from`] for the common case of a canister in `backend/`.
    fn first_symlink(root: &Utf8Path, rel: &str) -> Option<Utf8PathBuf> {
        first_symlink_from(root, &["backend"], rel)
    }

    #[test]
    fn plain_relative_path_has_no_symlink() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("backend/a/b")).unwrap();
        std::fs::write(root.join("backend/a/b/file.txt"), b"hi").unwrap();

        assert_eq!(first_symlink(root, "a/b"), None);
        assert_eq!(first_symlink(root, "a/b/file.txt"), None);
    }

    #[test]
    fn final_entry_is_symlink() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("backend/real")).unwrap();
        symlink(root.join("backend/real"), root.join("backend/link")).unwrap();

        // The reported path is relative to the sandbox root, so it names the
        // offending component without leaking the absolute on-disk location.
        assert_eq!(
            first_symlink(root, "link"),
            Some(Utf8PathBuf::from("backend/link"))
        );
    }

    #[test]
    fn intermediate_component_is_symlink() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // backend/real/inner exists; backend/link -> backend/real, so
        // "link/inner" traverses a symlink even though "inner" is a real dir.
        std::fs::create_dir_all(root.join("backend/real/inner")).unwrap();
        symlink(root.join("backend/real"), root.join("backend/link")).unwrap();

        // The reported path stops at the symlinked component rather than
        // continuing to the trailing real directory.
        assert_eq!(
            first_symlink(root, "link/inner"),
            Some(Utf8PathBuf::from("backend/link"))
        );
    }

    /// An entry that rises out of the canister directory is checked the whole
    /// way down from the root, so a symlink anywhere in the part it traverses
    /// is caught.
    #[test]
    fn symlink_outside_the_base_dir_is_caught() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("backend")).unwrap();
        std::fs::create_dir_all(root.join("real")).unwrap();
        symlink(root.join("real"), root.join("shared")).unwrap();

        assert_eq!(
            first_symlink(root, "../shared"),
            Some(Utf8PathBuf::from("shared"))
        );
    }

    #[test]
    fn missing_path_is_not_a_symlink() {
        let tmp = tempdir().unwrap();
        assert_eq!(first_symlink(tmp.path(), "does/not/exist"), None);
    }

    #[test]
    fn dot_components_are_ignored() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("backend/a")).unwrap();
        assert_eq!(first_symlink(root, "./a"), None);
    }

    #[test]
    fn symlinked_root_is_allowed() {
        // A symlink *above* the sandbox root is fine; only what the declared
        // entry traverses below the root is checked.
        let tmp = tempdir().unwrap();
        let real_root = tmp.path().join("real-root");
        std::fs::create_dir_all(real_root.join("backend/data")).unwrap();
        let linked_root = tmp.path().join("linked-root");
        symlink(&real_root, &linked_root).unwrap();

        assert_eq!(first_symlink(&linked_root, "data"), None);
    }

    /// The base directory's own ancestry is exempt as long as the entry does
    /// not rise out of it: a symlinked canister directory is how the project
    /// reaches its own canister, not something the manifest declared.
    #[test]
    fn symlinked_base_dir_is_allowed_for_an_entry_below_it() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("real-backend/data")).unwrap();
        symlink(root.join("real-backend"), root.join("backend")).unwrap();

        assert_eq!(first_symlink(root, "data"), None);
        // Rising out of it puts that ancestry back in scope, and it is rejected.
        assert_eq!(
            first_symlink(root, "../backend/data"),
            Some(Utf8PathBuf::from("backend"))
        );
    }

    /// An entry that rises out of the canister directory is checked from the
    /// root down, not just from where it re-anchored. Without that, a symlink
    /// above the canister's own directory would land the preopen outside the
    /// project — here `canisters` leads out of the project, so `../secrets`
    /// would otherwise open a directory the project does not contain.
    #[test]
    fn symlinked_ancestor_of_a_deep_base_dir_is_caught() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("elsewhere/backend")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere/secrets")).unwrap();
        symlink(root.join("elsewhere"), root.join("canisters")).unwrap();

        let base = &["canisters", "backend"];
        assert_eq!(
            first_symlink_from(root, base, "../secrets"),
            Some(Utf8PathBuf::from("canisters"))
        );
        // An entry that stays below the canister directory is unaffected: that
        // ancestry is how the project reaches the canister either way.
        assert_eq!(first_symlink_from(root, base, "data"), None);
    }
}
