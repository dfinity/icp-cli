#[cfg(unix)]
pub const PATH_SEPARATOR: &str = ":";

#[cfg(windows)]
pub const PATH_SEPARATOR: &str = ";";
