use clap::Subcommand;

pub(crate) mod args;
pub(crate) mod build;
pub(crate) mod canister;
pub(crate) mod cycles;
pub(crate) mod deploy;
pub(crate) mod environment;
pub(crate) mod identity;
pub(crate) mod network;
pub(crate) mod new;
pub(crate) mod parsers;
pub(crate) mod project;
pub(crate) mod settings;
pub(crate) mod sync;
pub(crate) mod token;

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    Build(build::BuildArgs),
    #[command(subcommand)]
    Canister(canister::Command),
    #[command(subcommand)]
    Cycles(cycles::Command),
    Deploy(deploy::DeployArgs),
    #[command(subcommand)]
    Environment(environment::Command),
    #[command(subcommand)]
    Identity(identity::Command),
    #[command(subcommand)]
    Network(network::Command),
    New(new::IcpGenerateArgs),
    #[command(subcommand)]
    Project(project::Command),
    Settings(settings::SettingsArgs),
    Sync(sync::SyncArgs),
    Token(token::Command),
}
