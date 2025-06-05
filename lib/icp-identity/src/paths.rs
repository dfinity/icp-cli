use camino::Utf8PathBuf;
use icp_dirs::IcpCliDirs;
use icp_fs::fs::{self, CreateDirAllError};

pub fn identity_defaults_path(dirs: &IcpCliDirs) -> Utf8PathBuf {
    dirs.identity_dir().join("identity_defaults.json")
}

pub fn ensure_identity_defaults_path(dirs: &IcpCliDirs) -> Result<Utf8PathBuf, CreateDirAllError> {
    let path = identity_defaults_path(dirs);
    fs::create_dir_all(path.parent().unwrap())?;
    Ok(path)
}

pub fn identity_list_path(dirs: &IcpCliDirs) -> Utf8PathBuf {
    dirs.identity_dir().join("identity_list.json")
}

pub fn ensure_identity_list_path(dirs: &IcpCliDirs) -> Result<Utf8PathBuf, CreateDirAllError> {
    let path = identity_list_path(dirs);
    fs::create_dir_all(path.parent().unwrap())?;
    Ok(path)
}

pub fn key_pem_path(dirs: &IcpCliDirs, name: &str) -> Utf8PathBuf {
    dirs.identity_dir().join(format!("keys/{name}.pem"))
}

pub fn ensure_key_pem_path(
    dirs: &IcpCliDirs,
    name: &str,
) -> Result<Utf8PathBuf, CreateDirAllError> {
    let path = key_pem_path(dirs, name);
    fs::create_dir_all(path.parent().unwrap())?;
    Ok(path)
}
