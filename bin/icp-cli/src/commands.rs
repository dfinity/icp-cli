use clap::{Parser, Subcommand};

use crate::{env::Env, error::AnyError};

mod identity;

#[derive(Parser)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand)]
enum Subcmd {
    Identity(identity::IdentityCmd),
}

pub fn dispatch(env: &Env, cmd: Cmd) -> Result<(), AnyError> {
    match cmd.subcmd {
        Subcmd::Identity(subcmd) => identity::dispatch(env, subcmd),
    }
}
