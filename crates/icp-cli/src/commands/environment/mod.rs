use clap::Subcommand;

pub(crate) mod list;

#[derive(Subcommand, Debug)]
pub enum Command {
    List(list::Cmd),
}
