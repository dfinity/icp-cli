use clap::Args;
use ic_management_canister_types::DeleteCanisterSnapshotArgs;

use crate::{
    commands::{
        Context, Mode,
        canister::snapshot::{CommandError, SnapshotId},
    },
    options::{EnvironmentOpt, IdentityOpt},
    store_id::Key,
};

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The name of the canister within the current project
    name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// The ID of the snapshot to delete.
    snapshot: SnapshotId,
}

pub async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            // Load project
            let p = ctx.project.load().await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            // Ensure canister is included in the environment
            if !env.canisters.contains_key(&args.name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: args.name.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: args.name.to_owned(),
            })?;

            // Management Interface
            let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

            // Delete snapshot
            mgmt.delete_canister_snapshot(
                &cid,
                &DeleteCanisterSnapshotArgs {
                    canister_id: cid,
                    snapshot_id: args.snapshot.0.clone(),
                },
            )
            .await?;

            eprintln!(
                "Deleted snapshot {} from canister '{}'",
                args.snapshot, args.name,
            );
        }
    }

    Ok(())
}
