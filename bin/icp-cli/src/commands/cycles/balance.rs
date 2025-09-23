use clap::Parser;
use snafu::Snafu;

use crate::context::{Context, ContextProjectError};

#[derive(Debug, Parser)]
pub struct Cmd;

pub async fn exec(_ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    unimplemented!()
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },
}
