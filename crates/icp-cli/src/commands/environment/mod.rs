use clap::Subcommand;

pub(crate) mod list;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    List(list::ListArgs),
}
