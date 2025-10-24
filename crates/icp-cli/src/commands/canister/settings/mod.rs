use clap::Subcommand;

pub(crate) mod show;
pub(crate) mod update;

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    Show(show::ShowArgs),
    Update(update::UpdateArgs),
}
