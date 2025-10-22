use std::io::{self, Write};

use candid::IDLArgs;
use clap::Args;
use dialoguer::console::Term;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, args},
    options::{IdentityOpt},
    store_id::{Key, LookupError},
};

#[derive(Args, Debug)]
pub(crate) struct CallArgs {
    /// Name of canister to call to
    pub(crate) canister: args::Canister,

    /// Name of canister method to call into
    pub(crate) method: String,

    /// String representation of canister call arguments
    pub(crate) args: String,

    #[arg(long)]
    pub(crate) network: Option<args::Network>,

    #[arg(long, default_value_t = args::Environment::default())]
    pub(crate) environment: args::Environment,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
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

    let (cid, agent) = match (args.canister.clone(), args.environment.clone(), args.network.clone()) {
        (_, args::Environment::Name(_), Some(_)) => {
            // Both an environment and a network are specified this is an error
            todo!()
        },
        (args::Canister::Name(_), args::Environment::Default(_), Some(_)) => {
            // This is not allowed, we should not use name with an environment not a network
            todo!()
        },
        (args::Canister::Name(cname), _, None) => {
            // A canister name was specified so we must be in a project

            // Get the environment name
            let ename = match &args.environment {
                args::Environment::Name(name) => name.clone(),
                args::Environment::Default(name) => name.clone(),
            };
            // Load project
            let p = ctx.project.load().await?;

            // Load target environment
            let env =
                p.environments
                    .get(&ename)
                    .ok_or(CommandError::EnvironmentNotFound {
                        name: ename.to_owned(),
                    })?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            // Ensure canister is included in the environment
            if !env.canisters.contains_key(&cname) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: cname.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: cname.to_owned(),
            })?;

            (cid, agent)
        },
        (args::Canister::Principal(principal), args::Environment::Name(env_name), None) => {
           // Call by canister_id to a network referenced by name
            todo!()

        },
        (args::Canister::Principal(principal), args::Environment::Default(_), Some(args::Network::Url(url))) => {
            // Make the call a canister id on some network

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Agent
            let agent = ctx.agent.create(id, &url).await?;

            (principal, agent)
        },
        (args::Canister::Principal(principal), args::Environment::Default(_), Some(args::Network::Name(_))) => {
            // Should handle known networks by name
            todo!()
        },
        (args::Canister::Principal(principal), args::Environment::Default(ename), None) => {
            // Load project
            let p = ctx.project.load().await?;


            // Load target environment
            let env =
                p.environments
                    .get(&ename)
                    .ok_or(CommandError::EnvironmentNotFound {
                        name: ename.to_owned(),
                    })?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            (principal, agent)
        },
    };


    // Parse candid arguments
    let cargs = candid_parser::parse_idl_args(&args.args)?;

    let res = agent
        .update(&cid, &args.method)
        .with_arg(cargs.to_bytes()?)
        .await?;

    let ret = IDLArgs::from_bytes(&res[..])?;

    print_candid_for_term(&mut Term::buffered_stdout(), &ret)?;

    Ok(())
}

/// Pretty-prints IDLArgs detecting the terminal's width to avoid the 80-column default.
pub(crate) fn print_candid_for_term(term: &mut Term, args: &IDLArgs) -> io::Result<()> {
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
