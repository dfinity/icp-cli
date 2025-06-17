use crate::env::Env;
use clap::Parser;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterCreateCmd {}

pub fn exec(_env: &Env, _cmd: CanisterCreateCmd) -> Result<(), CanisterCreateError> {
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCreateError {
    #[snafu(display("{error}"))]
    Unexpected { error: String },
}
