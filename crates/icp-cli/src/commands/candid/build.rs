use anyhow::{Context as _, bail};
use candid::Principal;
use candid::types::{Type, TypeInner};
use candid::{TypeEnv, types::Function};
use candid_parser::assist;
use candid_parser::utils::CandidSource;
use clap::{ArgGroup, Args, ValueEnum};
use ic_agent::Agent;
use icp::context::{Context, GetCanisterIdError, GetCanisterIdForEnvError};
use icp::prelude::*;
use icp::store_id::LookupIdError;
use std::io::Write;

use crate::commands::args;
use crate::operations::misc::fetch_canister_metadata;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Bin,
    Hex,
    Candid,
}

/// Interactively build Candid arguments for a canister method
#[derive(Args, Debug)]
#[command(group(ArgGroup::new("input").required(true).args(["canister", "candid_file"])))]
pub(crate) struct BuildArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::OptionalCanisterCommandArgs,

    /// Name of canister method to build arguments for.
    /// If not provided, an interactive prompt will be launched.
    pub(crate) method: Option<String>,

    /// Output file path. Pass `-` for stdout.
    #[arg(short, long)]
    pub(crate) output: PathBuf,

    /// Output format.
    #[arg(long, default_value = "bin")]
    pub(crate) format: OutputFormat,

    /// Optionally provide a local Candid file describing the canister interface,
    /// instead of looking it up from canister metadata.
    #[arg(short = 'c', long)]
    pub(crate) candid_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let interface = if let Some(path) = &args.candid_file {
        // the canister is optional if the candid file is provided
        load_candid_file(path)?
    } else {
        let agent = ctx
            .get_agent(
                &selections.identity,
                &selections.network,
                &selections.environment,
            )
            .await?;
        // otherwise, look up the canister
        let cid = match ctx
            .get_canister_id(
                &selections.canister.expect("required by arg group"),
                &selections.network,
                &selections.environment,
            )
            .await
        {
            Ok(cid) => cid,
            Err(GetCanisterIdError::GetCanisterIdForEnv {
                source:
                    GetCanisterIdForEnvError::CanisterIdLookup {
                        source,
                        canister_name,
                        environment_name,
                    },
            }) if matches!(*source, LookupIdError::IdNotFound { .. }) => {
                bail!(
                    "Canister {canister_name} has not been deployed in environment {environment_name}. This command requires an active deployment to reference"
                );
            }
            Err(e) => return Err(e).context("failed to look up canister ID"),
        };
        let Some(interface) = get_candid_type(&agent, cid).await else {
            bail!(
                "Could not fetch Candid interface from `candid:service` metadata section of canister {cid}"
            );
        };
        interface
    };

    let method = if let Some(method) = &args.method {
        method.clone()
    } else {
        pick_method(&interface, "Select a method")?
    };

    let Some(func) = interface.get_method(&method) else {
        bail!("method `{method}` not found in the canister's Candid interface");
    };

    let context = assist::Context::new(interface.env.clone());
    eprintln!("Build arguments for `{method}`:");
    let arguments = assist::input_args(&context, &func.args)?;

    let bytes = arguments
        .to_bytes_with_types(&interface.env, &func.args)
        .context("failed to serialize Candid arguments")?;

    if args.output.as_str() == "-" {
        match args.format {
            OutputFormat::Bin => {
                std::io::stdout().write_all(&bytes)?;
            }
            OutputFormat::Hex => {
                println!("{}", hex::encode(&bytes));
            }
            OutputFormat::Candid => {
                println!("{arguments}");
            }
        }
    } else {
        let path = &args.output;
        match args.format {
            OutputFormat::Bin => {
                icp::fs::write(path, &bytes)?;
            }
            OutputFormat::Hex => {
                icp::fs::write_string(path, &hex::encode(&bytes))?;
            }
            OutputFormat::Candid => {
                icp::fs::write_string(path, &format!("{arguments}\n"))?;
            }
        }
        _ = ctx.term.write_line(&format!("Written to {path}"));
    }

    Ok(())
}

/// Interactively pick a method from a canister's Candid interface.
pub(crate) fn pick_method(
    interface: &CanisterInterface,
    prompt: &str,
) -> Result<String, anyhow::Error> {
    let methods: Vec<&str> = interface.methods().collect();
    if methods.is_empty() {
        bail!("the canister's Candid interface has no methods");
    }
    let selection = dialoguer::Select::new()
        .with_prompt(prompt)
        .items(&methods)
        .default(0)
        .interact()?;
    Ok(methods[selection].to_string())
}

/// Gets the Candid type of a method on a canister by fetching its Candid interface.
///
/// This is a best effort function: it will succeed if
/// - the canister exposes its Candid interface in its metadata;
/// - the IDL file can be parsed and type checked in Rust parser;
/// - has an actor in the IDL file. If anything fails, it returns None.
pub(crate) async fn get_candid_type(
    agent: &Agent,
    canister_id: Principal,
) -> Option<CanisterInterface> {
    let candid_interface = fetch_canister_metadata(agent, canister_id, "candid:service").await?;
    let candid_source = CandidSource::Text(&candid_interface);
    let (type_env, ty) = candid_source.load().ok()?;
    let actor = ty?;
    Some(CanisterInterface {
        env: type_env,
        ty: actor,
    })
}

/// Loads a canister's Candid interface from a local `.did` file.
pub(crate) fn load_candid_file(path: &Path) -> Result<CanisterInterface, anyhow::Error> {
    let candid_source = CandidSource::File(path.as_std_path());
    let (type_env, ty) = candid_source
        .load()
        .with_context(|| format!("failed to load Candid file {path}"))?;
    let actor = ty.with_context(|| format!("Candid file {path} does not define a service"))?;
    Ok(CanisterInterface {
        env: type_env,
        ty: actor,
    })
}

pub(crate) struct CanisterInterface {
    pub(crate) env: TypeEnv,
    pub(crate) ty: Type,
}

impl CanisterInterface {
    pub(crate) fn methods(&self) -> impl Iterator<Item = &str> {
        let ty = if let TypeInner::Class(_, t) = &*self.ty.0 {
            t
        } else {
            &self.ty
        };
        let TypeInner::Service(methods) = &*ty.0 else {
            unreachable!("check_prog should verify service type")
        };
        methods.iter().map(|(name, _)| name.as_str())
    }
    pub(crate) fn get_method<'a>(&'a self, method_name: &'a str) -> Option<&'a Function> {
        self.env.get_method(&self.ty, method_name).ok()
    }
}
