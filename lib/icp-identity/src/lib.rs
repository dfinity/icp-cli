use camino::Utf8PathBuf;
use icp_fs::{fs, json};

use snafu::Snafu;

pub mod key;
pub mod manifest;
pub mod paths;
pub mod seed;

#[derive(Debug, Snafu)]
#[snafu(module(s_load))]
pub enum LoadIdentityError {
    #[snafu(transparent)]
    ReadFileError { source: fs::ReadToStringError },

    #[snafu(transparent)]
    LoadJsonError {
        source: icp_fs::json::LoadJsonFileError,
    },

    #[snafu(display("failed to load PEM file `{path}`: failed to parse"))]
    ParsePemError {
        path: Utf8PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("failed to load PEM file `{path}`: failed to decipher key"))]
    ParseKeyError {
        path: Utf8PathBuf,
        source: pkcs8::Error,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordError { message: String },

    #[snafu(display("file `{path}` was modified by an incompatible new version of icp-cli"))]
    BadVersion { path: Utf8PathBuf },
}

#[derive(Debug, Snafu)]
#[snafu(module(s_write))]
pub enum WriteIdentityError {
    #[snafu(transparent)]
    WriteFileError { source: fs::WriteFileError },

    #[snafu(transparent)]
    WriteJsonError { source: json::SaveJsonFileError },

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
