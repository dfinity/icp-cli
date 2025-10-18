use clap::Subcommand;

pub(crate) mod binding_env_vars;
pub(crate) mod call;
pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod info;
pub(crate) mod install;
pub(crate) mod list;
pub(crate) mod settings;
pub(crate) mod show;
pub(crate) mod snapshot;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod top_up;

#[derive(Debug, Subcommand)]
pub enum Command {
    Call(call::CallArgs),
    Create(create::CreateArgs),
    Delete(delete::DeleteArgs),
    Info(info::InfoArgs),
    Install(install::InstallArgs),
    List(list::ListArgs),

    #[command(subcommand)]
    Settings(settings::Command),

    Show(show::ShowArgs),

    #[command(subcommand)]
    Snapshot(snapshot::Command),

    Start(start::StartArgs),
    Status(status::StatusArgs),
    Stop(stop::StopArgs),
    TopUp(top_up::TopUpArgs),
}
