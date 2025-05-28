use clap::{Parser, Subcommand};

use crate::{env::Env, error::AnyError};

mod default;
mod import;
mod list;
mod new;
mod principal;

#[derive(Parser)]
pub struct IdentityCmd {
    #[command(subcommand)]
    subcmd: IdentitySubcmd,
}

#[derive(Subcommand)]
pub enum IdentitySubcmd {
    Import(import::ImportCmd),
    New(new::NewCmd),
    Principal(principal::PrincipalCmd),
    List(list::ListCmd),
    Default(default::DefaultCmd),
}

pub fn dispatch(env: &Env, cmd: IdentityCmd) -> Result<(), AnyError> {
    match cmd.subcmd {
        IdentitySubcmd::Import(subcmd) => env.print_result(import::exec(env, subcmd))?,
        IdentitySubcmd::New(subcmd) => env.print_result(new::exec(env, subcmd))?,
        IdentitySubcmd::Principal(subcmd) => env.print_result(principal::exec(env, subcmd))?,
        IdentitySubcmd::List(subcmd) => env.print_result(list::exec(env, subcmd))?,
        IdentitySubcmd::Default(subcmd) => env.print_result(default::exec(env, subcmd))?,
    }
    Ok(())
}

const DEFAULT_DERIVATION_PATH: &str = "m/44'/223'/0'/0/0";
