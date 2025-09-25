use clap::Parser;
use icp_project::{NoCanister, NoEnvironment};
use snafu::Snafu;

use crate::{
    context::{Context, ContextProjectError},
    options::EnvironmentOpt,
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest
    let pm = ctx.project()?;

    // Select canister to show
    let c = pm.canister(&cmd.name)?;

    // Load target environment
    let env = pm.environment(cmd.environment.name())?;

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&Key {
        network: network.to_owned(),
        environment: env.name.to_owned(),
        canister: c.name.to_owned(),
    })?;

    println!("{cid} => {c:?}");

    // TODO(or.ricon): Show canister details
    //  Things we might want to show (do we need to sub-command this?)
    //  - canister manifest (e.g resulting canister manifest after recipe definitions are processed)
    //  - canister deployment details (this canister is deployed to network X as part of environment Y)

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(transparent)]
    CanisterNotFound { source: NoCanister },

    #[snafu(transparent)]
    EnvironmentNotFound { source: NoEnvironment },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },
}
