use crate::env::Env;
use clap::Parser;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterInstallCmd {}

pub fn exec(_env: &Env, _cmd: CanisterInstallCmd) -> Result<(), CanisterInstallError> {
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterInstallError {
    #[snafu(display("{error}"))]
    Unexpected { error: String },
}
