use clap::Args;
use icp::context::Context;

use crate::{commands::args::ListArgsOptions, options::EnvironmentOpt};

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) options: ListArgsOptions,
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
    let environment_selection = args.environment.clone().into();
    let env = ctx.get_environment(&environment_selection).await?;

    if args.options.name_only {
        for c in env.canisters.keys() {
            ctx.term.write_line(c)?;
        }
        return Ok(());
    }

    if args.options.yaml_format {
        let yaml = serde_yaml::to_string(&env.canisters).expect("Serializing to yaml failed");
        ctx.term.write_line(&yaml)?;
        return Ok(());
    }

    Ok(())
}
