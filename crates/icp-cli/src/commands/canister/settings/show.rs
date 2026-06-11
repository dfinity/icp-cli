use clap::Args;
use ic_agent::export::Principal;
use ic_management_canister_types::{CanisterIdRecord, DefiniteCanisterSettings, LogVisibility};
use icp::context::Context;
use std::fmt::Write;

use crate::{commands::args::CanisterCommandArgs, operations::proxy_management};

/// Show the settings of a canister.
///
/// Queries the canister_status endpoint of the management canister and
/// displays only the settings fields. Requires the caller to be a controller.
#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    #[command(flatten)]
    pub(crate) cmd_args: CanisterCommandArgs,

    /// Format output as JSON
    #[arg(long = "json")]
    pub json_format: bool,

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    pub proxy: Option<Principal>,
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let result = proxy_management::canister_status(
        &agent,
        args.proxy,
        CanisterIdRecord { canister_id: cid },
    )
    .await?;

    let output = if args.json_format {
        serde_json::to_string(&result.settings).expect("Serializing settings to json failed")
    } else {
        build_output(&result.settings)
    };

    println!("{}", output.trim());
    Ok(())
}

fn build_output(s: &DefiniteCanisterSettings) -> String {
    let mut buf = String::new();
    writeln!(
        &mut buf,
        "Controllers: {}",
        s.controllers
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();
    writeln!(&mut buf, "Compute allocation: {}", s.compute_allocation).unwrap();
    writeln!(&mut buf, "Memory allocation: {}", s.memory_allocation).unwrap();
    writeln!(&mut buf, "Freezing threshold: {}", s.freezing_threshold).unwrap();
    writeln!(
        &mut buf,
        "Reserved cycles limit: {}",
        s.reserved_cycles_limit
    )
    .unwrap();
    writeln!(&mut buf, "Wasm memory limit: {}", s.wasm_memory_limit).unwrap();
    writeln!(
        &mut buf,
        "Wasm memory threshold: {}",
        s.wasm_memory_threshold
    )
    .unwrap();
    writeln!(&mut buf, "Log memory limit: {}", s.log_memory_limit).unwrap();

    let log_visibility = match &s.log_visibility {
        LogVisibility::Controllers => "Controllers".to_string(),
        LogVisibility::Public => "Public".to_string(),
        LogVisibility::AllowedViewers(viewers) => {
            if viewers.is_empty() {
                "Allowed viewers list is empty".to_string()
            } else {
                let mut v: Vec<String> = viewers.iter().map(|p| p.to_string()).collect();
                v.sort();
                format!("Allowed viewers: {}", v.join(", "))
            }
        }
    };
    writeln!(&mut buf, "Log visibility: {log_visibility}").unwrap();

    if s.environment_variables.is_empty() {
        writeln!(&mut buf, "Environment variables: N/A").unwrap();
    } else {
        writeln!(&mut buf, "Environment variables:").unwrap();
        for v in &s.environment_variables {
            writeln!(&mut buf, "  {}: {}", v.name, v.value).unwrap();
        }
    }

    buf
}
