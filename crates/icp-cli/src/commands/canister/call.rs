use std::io::{self, Write};

use candid::IDLArgs;
use clap::Args;
use dialoguer::console::Term;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, Mode, args},
    options::IdentityOpt,
    store_id::{Key, LookupError},
};

#[derive(Args, Debug)]
pub(crate) struct CallArgs {
    pub(crate) canister: args::Canister,

    /// Name of canister method to call into
    pub(crate) method: String,

    /// String representation of canister call arguments
    pub(crate) args: String,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[arg(long)]
    pub(crate) network: Option<args::Network>,

    #[arg(long)]
    pub(crate) environment: Option<args::Environment>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("an invalid argument was provided")]
    Args,

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

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

pub(crate) async fn exec(ctx: &Context, args: &CallArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            let args::Canister::Principal(_) = &args.canister else {
                return Err(CommandError::Args);
            };

            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(pdir) => {
            let args::Canister::Name(name) = &args.canister else {
                return Err(CommandError::Args);
            };

            // Argument (Environment)
            let args::Environment::Name(env) = args.environment.clone().unwrap_or_default();

            // Load project
            let p = ctx.project.load(pdir).await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p
                .environments
                .get(&env)
                .ok_or(CommandError::EnvironmentNotFound { name: env })?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            // Ensure canister is included in the environment
            if !env.canisters.contains_key(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            })?;

            // Parse candid arguments
            let cargs = candid_parser::parse_idl_args(&args.args)?;

            let res = agent
                .update(&cid, &args.method)
                .with_arg(cargs.to_bytes()?)
                .await?;

            let ret = IDLArgs::from_bytes(&res[..])?;

            print_candid_for_term(&mut Term::buffered_stdout(), &ret)?;
        }
    }

    Ok(())
}

/// Pretty-prints IDLArgs detecting the terminal's width to avoid the 80-column default.
fn print_candid_for_term(term: &mut Term, args: &IDLArgs) -> io::Result<()> {
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
