use clap::Args;

use crate::commands::{
    Context,
    args::{ArgValidationError, CanisterEnvironmentArgs},
};

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    #[command(flatten)]
    pub(crate) cmd_args: CanisterEnvironmentArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Shared(#[from] ArgValidationError),
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), CommandError> {
    let cid = args.cmd_args.get_cid_for_environment(ctx).await?;

    println!("{cid} => {}", args.cmd_args.canister);

    // TODO(or.ricon): Show canister details
    //  Things we might want to show (do we need to sub-command this?)
    //  - canister manifest (e.g resulting canister manifest after recipe definitions are processed)
    //  - canister deployment details (this canister is deployed to network X as part of environment Y)

    Ok(())
}
