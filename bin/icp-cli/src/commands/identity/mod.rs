use clap::{Parser, Subcommand};

use crate::{env::Env, error::AnyError};

mod default;
mod get_principal;
mod import;
mod list;
mod new;

#[derive(Parser)]
pub struct IdentityCmd {
    #[command(subcommand)]
    subcmd: IdentitySubcmd,
}

#[derive(Subcommand)]
pub enum IdentitySubcmd {
    Import(import::ImportCmd),
    New(new::NewCmd),
    GetPrincipal(get_principal::GetPrincipalCmd),
    List(list::ListCmd),
    Default(default::DefaultCmd),
}

pub fn dispatch(env: &Env, cmd: IdentityCmd) -> Result<(), AnyError> {
    match cmd.subcmd {
        IdentitySubcmd::Import(subcmd) => env.print_result(import::exec(env, subcmd))?,
        IdentitySubcmd::New(subcmd) => env.print_result(new::exec(env, subcmd))?,
        IdentitySubcmd::GetPrincipal(subcmd) => {
            env.print_result(get_principal::exec(env, subcmd))?
        }
        IdentitySubcmd::List(subcmd) => env.print_result(list::exec(env, subcmd))?,
        IdentitySubcmd::Default(subcmd) => env.print_result(default::exec(env, subcmd))?,
    }
    Ok(())
}

const DEFAULT_DERIVATION_PATH: &str = "m/44'/223'/0'/0/0";
