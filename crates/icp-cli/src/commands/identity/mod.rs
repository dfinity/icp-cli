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

/// Manage your identities
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    AccountId(account_id::AccountIdArgs),
    Default(default::DefaultArgs),
    Delete(delete::DeleteArgs),
    Export(export::ExportArgs),
    Import(import::ImportArgs),
    #[command(subcommand)]
    Link(link::Command),
    List(list::ListArgs),
    New(new::NewArgs),
    Principal(principal::PrincipalArgs),
    Rename(rename::RenameArgs),
}

#[derive(Debug, Clone, ValueEnum, Default)]
enum StorageMode {
    Plaintext,
    #[default]
    Keyring,
    Password,
}
