use clap::Args;
use icp::context::Context;

use crate::commands::{
    args,
    canister::status::{self, StatusArgs, StatusArgsOptions},
};

/// Show the status of a canister.
///
/// By default this queries the status endpoint of the management canister.
/// If the caller is not a controller, falls back on fetching public
/// information from the state tree.
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
