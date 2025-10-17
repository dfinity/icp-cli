use lazy_static::lazy_static;

lazy_static! {
    static ref VERSION_STR: String = env!("CARGO_PKG_VERSION").to_string();
    static ref GIT_SHA: String = env!("GIT_SHA").to_string();
}

/// Returns the version of icp-cli that was built.
/// In debug, add a timestamp of the upstream compilation at the end of version to ensure all
/// debug runs are unique.
pub fn icp_cli_version_str() -> &'static str {
    &VERSION_STR
}

/// Returns the git sha of the build.
pub fn git_sha() -> &'static str {
    &GIT_SHA
}
