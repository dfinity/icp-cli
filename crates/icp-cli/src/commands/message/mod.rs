use clap::Subcommand;

pub(crate) mod send;

/// Work with signed messages
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Send(send::SendArgs),
}
