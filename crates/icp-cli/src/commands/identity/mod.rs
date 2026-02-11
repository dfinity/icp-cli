use clap::{Subcommand, ValueEnum};

pub(crate) mod account_id;
pub(crate) mod default;
pub(crate) mod delete;
pub(crate) mod export;
pub(crate) mod import;
pub(crate) mod link;
pub(crate) mod list;
pub(crate) mod new;
pub(crate) mod principal;
pub(crate) mod rename;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Display the ICP ledger and ICRC-1 account identifiers for the current identity
    AccountId(account_id::AccountIdArgs),

    /// Display the currently selected identity
    Default(default::DefaultArgs),

    /// Delete an identity
    Delete(delete::DeleteArgs),

    /// Print the PEM file for the identity
    Export(export::ExportArgs),

    /// Import a new identity
    Import(import::ImportArgs),

    /// Link an external key to a new identity
    #[command(subcommand)]
    Link(link::Command),

    /// List the identities
    List(list::ListArgs),

    /// Create a new identity
    New(new::NewArgs),

    /// Display the principal for the current identity
    Principal(principal::PrincipalArgs),

    /// Rename an identity
    Rename(rename::RenameArgs),
}

#[derive(Debug, Clone, ValueEnum, Default)]
enum StorageMode {
    Plaintext,
    #[default]
    Keyring,
    Password,
}
