use clap::Parser;
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn dispatch(_cmd: Cmd) -> Result<(), BuildCommandError> {
    Ok(())
}

#[derive(Debug, Snafu)]
#[snafu(display("Failed to build canister"))]
pub struct BuildCommandError {}
