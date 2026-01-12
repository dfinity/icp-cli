use clap::{Subcommand, ValueEnum};

pub(crate) mod default;
pub(crate) mod import;
pub(crate) mod list;
pub(crate) mod new;
pub(crate) mod principal;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Display the currently selected identity
    Default(default::DefaultArgs),

    /// Import a new identity
    Import(import::ImportArgs),

    /// List the identities
    List(list::ListArgs),

    /// Create a new identity
    New(new::NewArgs),

    /// Display the principal for the current identity
    Principal(principal::PrincipalArgs),
}

#[derive(Debug, Clone, ValueEnum, Default)]
enum StorageMode {
    Plaintext,
    #[default]
    Keyring,
    Password,
}
