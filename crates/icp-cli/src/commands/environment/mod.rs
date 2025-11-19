use clap::Subcommand;

pub(crate) mod list;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Display a list of enviroments
    List(list::ListArgs),
}
