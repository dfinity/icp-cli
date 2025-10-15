use clap::Subcommand;

pub(crate) mod default;
pub(crate) mod import;
pub(crate) mod list;
pub(crate) mod new;
pub(crate) mod principal;

#[derive(Debug, Subcommand)]
pub enum Command {
    Default(default::DefaultArgs),
    Import(import::ImportArgs),
    List(list::ListArgs),
    New(new::NewArgs),
    Principal(principal::PrincipalArgs),
}
