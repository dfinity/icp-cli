use clap::Args;
use icp::context::Context;
use crate::commands::args::ListArgsOptions;

#[derive(Args, Debug)]
pub(crate) struct ListArgs {

    #[command(flatten)]
    pub(crate) options: ListArgsOptions,

}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
    // Load project
    let pm = ctx.project.load().await?;

    // List networks
    if args.options.name_only {
        for e in pm.networks.keys() {
            ctx.term.write_line(e)?;
        }
        return Ok(());
    }

    if args.options.yaml_format {
        let yaml = serde_yaml::to_string(&pm.networks).expect("Serializing to yaml failed");
        ctx.term.write_line(&yaml)?;
        return Ok(());
    }

    Ok(())
}
