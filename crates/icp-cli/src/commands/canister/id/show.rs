use std::collections::BTreeMap;
use std::io::stdout;

use clap::Args;
use icp::context::{CanisterSelection, Context, EnvironmentSelection, GetCanisterIdForEnvError};
use icp::store_id::LookupIdError;
use serde::Serialize;

use crate::options::EnvironmentOpt;

/// Show canister IDs in an environment.
///
/// When a canister name is given, prints its ID. Without a name, lists all
/// canisters with their ID or "(not set)".
#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    /// Name of the canister as defined in icp.yaml. If omitted, lists all
    /// canisters with their ID or "(not set)".
    canister: Option<String>,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// Output as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct JsonOutput {
    canister: String,
    canister_id: String,
    environment: String,
}

#[derive(Serialize)]
struct JsonAllEntry {
    canister: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    canister_id: Option<String>,
}

#[derive(Serialize)]
struct JsonAllOutput {
    environment: String,
    canisters: Vec<JsonAllEntry>,
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), anyhow::Error> {
    let environment: EnvironmentSelection = args.environment.clone().into();

    if let Some(canister) = &args.canister {
        let selection = CanisterSelection::Named(canister.clone());
        let canister_id = ctx
            .get_canister_id_for_env(&selection, &environment)
            .await?;

        if args.json {
            serde_json::to_writer(
                stdout(),
                &JsonOutput {
                    canister: canister.clone(),
                    canister_id: canister_id.to_string(),
                    environment: environment.name().to_string(),
                },
            )?;
        } else {
            println!("{canister_id}");
        }
    } else {
        let env = ctx.get_environment(&environment).await?;
        let mut entries: BTreeMap<String, Option<String>> = BTreeMap::new();
        for name in env.canisters.keys() {
            let sel = CanisterSelection::Named(name.clone());
            match ctx.get_canister_id_for_env(&sel, &environment).await {
                Ok(id) => {
                    entries.insert(name.clone(), Some(id.to_string()));
                }
                Err(GetCanisterIdForEnvError::CanisterIdLookup { source, .. })
                    if matches!(*source, LookupIdError::IdNotFound { .. }) =>
                {
                    entries.insert(name.clone(), None);
                }
                Err(e) => return Err(e.into()),
            }
        }

        if args.json {
            let canisters = entries
                .into_iter()
                .map(|(name, id)| JsonAllEntry {
                    canister: name,
                    canister_id: id,
                })
                .collect();
            serde_json::to_writer(
                stdout(),
                &JsonAllOutput {
                    environment: environment.name().to_string(),
                    canisters,
                },
            )?;
        } else {
            for (name, id) in &entries {
                match id {
                    Some(id) => println!("{name}\t{id}"),
                    None => println!("{name}\t(not set)"),
                }
            }
        }
    }

    Ok(())
}
