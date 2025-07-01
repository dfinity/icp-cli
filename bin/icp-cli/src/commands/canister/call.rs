use crate::env::EnvGetAgentError;
use crate::options::NetworkOpt;
use crate::{candid::print_candid_for_term, env::Env};
use candid_parser::IDLArgs;
use clap::Parser;
use dialoguer::console::Term;
use icp_identity::key::LoadIdentityInContextError;
use snafu::{ResultExt, Snafu};

#[derive(Parser, Debug)]
pub struct CanisterCallCmd {
    #[clap(flatten)]
    network: NetworkOpt,

    pub canister: String,
    pub method: String,
    pub args: String,
}

pub async fn exec(env: &Env, cmd: CanisterCallCmd) -> Result<(), CanisterCallError> {
    env.set_network_opt(cmd.network);

    let agent = env.agent()?;

    let canister_id = if let Ok(principal) = cmd.canister.parse() {
        principal
    } else {
        env.id_store.lookup(&cmd.canister)?
    };
    let args = candid_parser::parse_idl_args(&cmd.args).context(DecodeArgsSnafu)?;
    let arg_bytes = args.to_bytes().context(EncodeArgsSnafu)?;
    let res = agent
        .update(&canister_id, &cmd.method)
        .with_arg(arg_bytes)
        .await?;

    let ret = IDLArgs::from_bytes(&res[..]).context(DecodeReturnSnafu)?;

    print_candid_for_term(&mut Term::buffered_stdout(), &ret).context(WriteTermSnafu)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCallError {
    #[snafu(transparent)]
    EnvCreateAgent { source: EnvGetAgentError },

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

    #[snafu(display("failed to print candid return value"))]
    WriteTermError { source: std::io::Error },

    #[snafu(transparent)]
    CallError { source: ic_agent::AgentError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },
}
