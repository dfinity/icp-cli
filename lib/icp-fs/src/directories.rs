use directories::ProjectDirs;

fn tool_dirs() -> ProjectDirs {
    ProjectDirs::from("org", "DFINITY Stiftung", "icp-cli").unwrap()
}

pub fn cache_dir() -> std::path::PathBuf {
    tool_dirs().cache_dir().to_path_buf()
}
