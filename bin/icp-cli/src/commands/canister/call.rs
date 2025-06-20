use std::io::stdout;

use candid_parser::IDLArgs;
use clap::Parser;
use dialoguer::console::Term;
use ic_agent::Agent;
use icp_identity::key::LoadIdentityInContextError;
use snafu::{ResultExt, Snafu};

use crate::env::Env;

#[derive(Parser, Debug)]
pub struct CanisterCallCmd {
    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://localhost:8000")]
    network_url: String,
    pub canister: String,
    pub method: String,
    pub args: String,
}

pub async fn exec(env: &Env, cmd: CanisterCallCmd) -> Result<(), CanisterCallError> {
    let identity = env.load_identity()?;

    let agent = Agent::builder()
        .with_url(&cmd.network_url)
        .with_arc_identity(identity)
        .build()?;

    if cmd.network_url.contains("127.0.0.1") || cmd.network_url.contains("localhost") {
        agent.fetch_root_key().await?;
    }
    let canister_id = if let Ok(principal) = cmd.canister.parse() {
        principal
    } else {
        env.id_store.lookup(&cmd.canister)?
    };
    let args = candid_parser::parse_idl_args(&cmd.args).context(DecodeArgsSnafu)?;
    let arg_bytes = args.to_bytes().context(EncodeArgsSnafu)?;
    let agent = Agent::builder().with_url(&cmd.network_url).build().unwrap();
    agent.fetch_root_key().await.unwrap();
    let res = agent
        .update(&canister_id, &cmd.method)
        .with_arg(arg_bytes)
        .await?;
    let ret = IDLArgs::from_bytes(&res[..]).context(DecodeReturnSnafu)?;

    let term = Term::stdout();
    if term.is_term() {
        let width = term.size().1 as usize;
        let pp_args = candid_parser::pretty::candid::value::pp_args(&ret);
        match pp_args.render(width, &mut stdout().lock()) {
            Ok(()) => {
                println!();
            }
            Err(_) => {
                println!("{ret}")
            }
        }
    } else {
        println!("{ret}")
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCallError {
    #[snafu(transparent)]
    LookupError {
        source: crate::store_id::LookupError,
    },
    #[snafu(display("failed to parse candid arguments"))]
    DecodeArgsError { source: candid_parser::Error },
    #[snafu(display("failed to serialize candid arguments"))]
    EncodeArgsError { source: candid::Error },
    #[snafu(display("failed to decode candid return value"))]
    DecodeReturnError { source: candid::Error },
    #[snafu(transparent)]
    CallError { source: ic_agent::AgentError },
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },
}
