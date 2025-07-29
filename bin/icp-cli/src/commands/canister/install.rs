use clap::Parser;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::InstallMode;
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_artifact::LookupError as LookupArtifactError,
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // Choose canisters to install
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = &cmd.name {
        if cs.is_empty() {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
        }
    }

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Filter for environment canisters
    let cs = cs
        .iter()
        .filter(|(_, c)| ecs.contains(&c.name))
        .collect::<Vec<_>>();

    // Ensure canister is included in the environment
    if let Some(name) = &cmd.name {
        if !ecs.contains(name) {
            return Err(CommandError::EnvironmentCanister {
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            });
        }
    }

    // Ensure at least one canister has been selected
    if cs.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    for (_, c) in cs {
        // Lookup the canister id
        let cid = ctx.id_store.lookup(&Key {
            network: network.to_owned(),
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        })?;

        // Lookup the canister build artifact
        let wasm = ctx.artifact_store.lookup(&c.name)?;

        // Retrieve canister status
        let (status,) = mgmt.canister_status(&cid).await?;

        let install_mode = match cmd.mode.as_ref() {
            // Auto
            "auto" => match status.module_hash {
                // Canister has had code installed to it.
                Some(_) => InstallMode::Upgrade(None),

                // Canister has not had code installed to it.
                None => InstallMode::Install,
            },

            // Install
            "install" => InstallMode::Install,

            // Reinstall
            "reinstall" => InstallMode::Reinstall,

            // Upgrade
            "upgrade" => InstallMode::Upgrade(None),

            // invalid
            _ => panic!("invalid install mode"),
        };

        // Install code to canister
        mgmt.install_code(&cid, &wasm)
            .with_mode(install_mode)
            .await?;

        eprintln!(
            "Installed WASM payload to canister '{}' (ID: '{}')",
            c.name, cid,
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to install"))]
    NoCanisters,

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    LookupCanisterArtifact { source: LookupArtifactError },

    #[snafu(transparent)]
    InstallAgent { source: AgentError },
}
