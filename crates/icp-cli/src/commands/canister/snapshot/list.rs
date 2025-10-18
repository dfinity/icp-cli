use clap::Args;
use ic_agent::AgentError;
use ic_management_canister_types::Snapshot;
use icp::{agent, identity, network};
use indicatif::HumanBytes;
use time::{OffsetDateTime, macros::format_description};

use crate::{
    commands::{Context, Mode, canister::snapshot::SnapshotId},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub struct ListArgs {
    /// The name of the canister within the current project
    name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Status(#[from] AgentError),
}

pub async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), CommandError> {
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

            let (snapshots,) = mgmt.list_canister_snapshots(&cid).await?;

            if snapshots.is_empty() {
                eprintln!("No snapshots found for canister '{}'", args.name);
            } else {
                for snapshot in snapshots {
                    print_snapshot(&snapshot);
                }
            }
        }
    }

    Ok(())
}

fn print_snapshot(snapshot: &Snapshot) {
    let time_fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");

    eprintln!(
        "{}: {}, taken at {}",
        SnapshotId(snapshot.id.clone()),
        HumanBytes(snapshot.total_size),
        OffsetDateTime::from_unix_timestamp_nanos(snapshot.taken_at_timestamp as i128)
            .unwrap()
            .format(time_fmt)
            .unwrap()
    );
}
