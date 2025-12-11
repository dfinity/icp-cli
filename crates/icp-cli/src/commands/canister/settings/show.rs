use clap::Args;
use icp::context::Context;

use crate::commands::{
    args,
    canister::status::{self, StatusArgs, StatusArgsOptions},
};

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    /// canister name or principal to target.
    /// When using a name, an enviroment must be specified.
    pub(crate) canister: args::Canister,

    #[command(flatten)]
    pub(crate) options: StatusArgsOptions,
}

impl From<&ShowArgs> for StatusArgs {
    fn from(value: &ShowArgs) -> Self {
        StatusArgs {
            canister: Some(value.canister.clone()),
            options: value.options.clone(),
        }
    }
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), anyhow::Error> {
    let args: StatusArgs = args.into();
    status::exec(ctx, &args).await
}
