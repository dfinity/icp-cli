use clap::Subcommand;

pub(crate) mod default;
pub(crate) mod import;
pub(crate) mod list;
pub(crate) mod new;
pub(crate) mod principal;

#[derive(Debug, Subcommand)]
pub enum Command {
    Default(default::DefaultCmd),
    Import(import::ImportCmd),
    List(list::ListCmd),
    New(new::NewCmd),
    Principal(principal::PrincipalCmd),
}
