use clap::Subcommand;

pub(crate) mod request;
pub(crate) mod sign;
pub(crate) mod r#use;

/// Manage delegations for identities
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Request(request::RequestArgs),
    Sign(sign::SignArgs),
    Use(r#use::UseArgs),
}
