use std::io::{self, Write};

use candid::IDLArgs;
use clap::Parser;
use dialoguer::console::Term;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, ContextError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError},
};

#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of canister to call to
    pub name: String,

    /// Name of canister method to call into
    pub method: String,

    /// String representation of canister call arguments
    pub args: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    EnvironmentNotFound(#[from] ContextError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupError),

    #[error("failed to parse candid arguments")]
    DecodeArgsError(#[from] candid_parser::Error),

    #[error("failed to serialize candid arguments")]
    Candid(#[from] candid::Error),

    #[error("failed to print candid return value")]
    WriteTermError(#[from] std::io::Error),

    #[error(transparent)]
    Call(#[from] ic_agent::AgentError),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the environment
    let env = ctx.get_environment(cmd.environment.name()).await?;

    // Agent
    let agent = ctx.get_agent(&env, cmd.identity.into()).await?;

    // Ensure canister is included in the environment
    if !env.canisters.contains_key(&cmd.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: cmd.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.ids.lookup(&Key {
        network: env.network.name.to_owned(),
        environment: env.name.to_owned(),
        canister: cmd.name.to_owned(),
    })?;

    // Parse candid arguments
    let args = candid_parser::parse_idl_args(&cmd.args)?;

    let res = agent
        .update(&cid, &cmd.method)
        .with_arg(args.to_bytes()?)
        .await?;

    let ret = IDLArgs::from_bytes(&res[..])?;

    print_candid_for_term(&mut Term::buffered_stdout(), &ret)?;

    Ok(())
}

/// Pretty-prints IDLArgs detecting the terminal's width to avoid the 80-column default.
pub fn print_candid_for_term(term: &mut Term, args: &IDLArgs) -> io::Result<()> {
    if term.is_term() {
        let width = term.size().1 as usize;
        let pp_args = candid_parser::pretty::candid::value::pp_args(args);
        match pp_args.render(width, term) {
            Ok(()) => {
                writeln!(term)?;
            }
            Err(_) => {
                writeln!(term, "{args}")?;
            }
        }
    } else {
        writeln!(term, "{args}")?;
    }
    term.flush()?;
    Ok(())
}
