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
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod top_up;

#[derive(Debug, Subcommand)]
pub enum Command {
    Call(call::Cmd),
    Create(create::Cmd),
    Delete(delete::Cmd),
    Info(info::Cmd),
    Install(install::Cmd),
    List(list::Cmd),
    Show(show::Cmd),
    Start(start::Cmd),
    Status(status::Cmd),
    Stop(stop::Cmd),
    TopUp(top_up::Cmd),

    #[command(subcommand)]
    Settings(settings::Command),
}
