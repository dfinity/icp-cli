use icp_fs::fs;

use snafu::Snafu;
use std::path::PathBuf;

pub mod key;
pub mod manifest;

#[derive(Debug, Snafu)]
#[snafu(module(s_load))]
pub enum LoadIdentityError {
    #[snafu(display("failed to write configuration defaults"))]
    WriteDefaultsError { source: WriteIdentityError },

    #[snafu(transparent)]
    ReadFileError { source: fs::ReadFileError },

    #[snafu(display("failed to parse json at `{}`", path.display()))]
    ParseJsonError {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(display("failed to load PEM file `{}`: failed to parse", path.display()))]
    ParsePemError {
        path: PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("failed to load PEM file `{}`: failed to decipher key", path.display()))]
    ParseKeyError { path: PathBuf, source: pkcs8::Error },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordError { message: String },

    #[snafu(display("file {} was modified by an incompatible new version of icp-cli", path.display()))]
    BadVersion { path: PathBuf },
}

#[derive(Debug, Snafu)]
#[snafu(module(s_write))]
pub enum WriteIdentityError {
    #[snafu(transparent)]
    WriteFileError { source: fs::WriteFileError },

    #[snafu(transparent)]
    CreateDirectoryError { source: fs::CreateDirAllError },
}

#[derive(Debug, Snafu)]
#[snafu(module(s_create))]
pub enum CreateIdentityError {
    #[snafu(transparent)]
    Load { source: LoadIdentityError },

    #[snafu(transparent)]
    Write { source: WriteIdentityError },

    #[snafu(display("identity `{name}` already exists"))]
    IdentityAlreadyExists { name: String },
}
