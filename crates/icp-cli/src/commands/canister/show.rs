use clap::Args;

use icp::context::{CanisterSelection, Context, EnvironmentSelection};

use crate::commands::args::CanisterEnvironmentArgs;

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    #[command(flatten)]
    pub(crate) cmd_args: CanisterEnvironmentArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    GetCanisterId(#[from] icp::context::GetCanisterIdError),
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), CommandError> {
    let canister_selection: CanisterSelection = args.cmd_args.canister.clone().into();
    let environment_selection: EnvironmentSelection =
        args.cmd_args.environment.clone().unwrap_or_default().into();

    let cid = ctx
        .get_canister_id(&canister_selection, &environment_selection)
        .await?;

    println!("{cid} => {}", args.cmd_args.canister);

    // TODO(or.ricon): Show canister details
    //  Things we might want to show (do we need to sub-command this?)
    //  - canister manifest (e.g resulting canister manifest after recipe definitions are processed)
    //  - canister deployment details (this canister is deployed to network X as part of environment Y)

    Ok(())
}
