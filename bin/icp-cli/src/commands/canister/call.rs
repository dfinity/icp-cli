use std::io::{self, Write};

use candid::IDLArgs;
use clap::Parser;
use dialoguer::console::Term;
use icp_identity::key::LoadIdentityInContextError;
use snafu::{ResultExt, Snafu};

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of canister to call to
    pub name: String,

    /// Name of canister method to call into
    pub method: String,

    /// String representation of canister call arguments
    pub args: String,

    #[clap(flatten)]
    identity: IdentityOpt,

    #[clap(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be synced.
    let pm = ctx.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CommandError::CanisterNotFound { name: cmd.name })?;

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

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&c.name)?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    ctx.require_network(
        env.network
            .as_ref()
            .expect("no network specified in environment"),
    );

    // Prepare agent
    let agent = ctx.agent()?;

    // Parse candid arguments
    let args = candid_parser::parse_idl_args(&cmd.args).context(DecodeArgsSnafu)?;

    let res = agent
        .update(&cid, &cmd.method)
        .with_arg(args.to_bytes().context(EncodeArgsSnafu)?)
        .await?;

    let ret = IDLArgs::from_bytes(&res[..]).context(DecodeReturnSnafu)?;

    print_candid_for_term(&mut Term::buffered_stdout(), &ret).context(WriteTermSnafu)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupError {
        source: crate::store_id::LookupError,
    },

    #[snafu(transparent)]
    CreateAgent { source: ContextGetAgentError },

    #[snafu(display("failed to parse candid arguments"))]
    DecodeArgsError { source: candid_parser::Error },

    #[snafu(display("failed to serialize candid arguments"))]
    EncodeArgsError { source: candid::Error },

    #[snafu(display("failed to decode candid return value"))]
    DecodeReturnError { source: candid::Error },

    #[snafu(display("failed to print candid return value"))]
    WriteTermError { source: std::io::Error },

    #[snafu(transparent)]
    CallError { source: ic_agent::AgentError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },
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
