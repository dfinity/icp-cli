//! Reducing the paths a plugin step declared to the ones that have to be acted
//! on.
//!
//! A step's `dirs`/`files` entries are configuration as much as they are a
//! grant: the same tree may legitimately be declared under several keys, or a
//! tree and a subtree of it. Both the runtime that preopens them for a plugin
//! and the bundler that copies them into an archive want the reduced set,
//! while the declared list is still passed along whole.

use std::collections::HashSet;

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
mod tests {
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
