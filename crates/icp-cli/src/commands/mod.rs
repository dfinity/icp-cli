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
    /// Build canisters
    Build(build::BuildArgs),

    /// Perform canister operations against a network
    #[command(subcommand)]
    Canister(canister::Command),

    /// Mint and manage cycles
    #[command(subcommand)]
    Cycles(cycles::Command),

    /// Deploy a project to an environment
    Deploy(deploy::DeployArgs),

    /// Show information about the current project environments
    #[command(subcommand)]
    Environment(environment::Command),

    /// Manage your identities
    #[command(subcommand)]
    Identity(identity::Command),

    /// Launch and manage local test networks
    #[command(subcommand)]
    Network(network::Command),

    /// Create a new ICP project from a template
    ///
    /// Under the hood templates are generated with `cargo-generate`.
    /// See the cargo-generate docs for a guide on how to write your own templates:
    /// https://docs.rs/cargo-generate/0.23.7/cargo_generate/
    New(new::IcpGenerateArgs),

    /// Display information about the current project
    #[command(subcommand)]
    Project(project::Command),

    /// Configure user settings
    Settings(settings::SettingsArgs),

    /// Synchronize canisters
    Sync(sync::SyncArgs),

    /// Perform token transactions
    Token(token::Command),
}
