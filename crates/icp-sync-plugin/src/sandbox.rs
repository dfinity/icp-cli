use camino::{Utf8Path, Utf8PathBuf};

/// Returns `true` iff `path` (already canonicalized) starts with at least one
/// of the `allowed_dirs`.
pub fn is_path_allowed(path: &Utf8Path, allowed_dirs: &[Utf8PathBuf]) -> bool {
    allowed_dirs.iter().any(|dir| path.starts_with(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(paths: &[&str]) -> Vec<Utf8PathBuf> {
        paths.iter().map(|p| Utf8PathBuf::from(*p)).collect()
    }

    fn path(s: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(s)
    }

    #[test]
    fn allowed_exact_dir() {
        let allowed = dirs(&["/project/canister/assets"]);
        assert!(is_path_allowed(
            path("/project/canister/assets").as_path(),
            &allowed
        ));
    }

    #[test]
    fn allowed_file_inside_dir() {
        let allowed = dirs(&["/project/canister/assets"]);
        assert!(is_path_allowed(
            path("/project/canister/assets/data.txt").as_path(),
            &allowed
        ));
    }

    #[test]
    fn allowed_nested_file() {
        let allowed = dirs(&["/project/canister/assets"]);
        assert!(is_path_allowed(
            path("/project/canister/assets/subdir/data.txt").as_path(),
            &allowed
        ));
    }

    #[test]
    fn denied_outside_dir() {
        let allowed = dirs(&["/project/canister/assets"]);
        assert!(!is_path_allowed(
            path("/project/canister/other/data.txt").as_path(),
            &allowed
        ));
    }

    #[test]
    fn denied_parent_traversal_attempt() {
        // A path that looks like it goes outside — canonicalization in the
        // host prevents this from reaching is_path_allowed in practice, but
        // verify we handle an already-resolved traversal correctly.
        let allowed = dirs(&["/project/canister/assets"]);
        assert!(!is_path_allowed(path("/etc/passwd").as_path(), &allowed));
    }

    #[test]
    fn denied_sibling_prefix_match() {
        // "/project/canister/assets-other" must NOT be allowed just because
        // "/project/canister/assets" is in the list.
        let allowed = dirs(&["/project/canister/assets"]);
        assert!(!is_path_allowed(
            path("/project/canister/assets-other/file.txt").as_path(),
            &allowed
        ));
    }

    #[test]
    fn multiple_allowed_dirs() {
        let allowed = dirs(&["/project/canister/assets", "/project/canister/config"]);
        assert!(is_path_allowed(
            path("/project/canister/config/settings.json").as_path(),
            &allowed
        ));
        assert!(!is_path_allowed(
            path("/project/canister/private/secret.key").as_path(),
            &allowed
        ));
    }
}
