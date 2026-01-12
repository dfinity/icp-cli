use clap::Args;
use icp::context::Context;
use icp::identity::manifest::{IdentityDefaults, IdentityList, change_default_identity};

#[derive(Debug, Args)]
pub(crate) struct DefaultArgs {
    /// Identity to set as default. If omitted, prints the current default.
    name: Option<String>,
}

pub(crate) async fn exec(ctx: &Context, args: &DefaultArgs) -> Result<(), anyhow::Error> {
    // Load project directories
    let dirs = ctx.dirs.identity()?;

    match &args.name {
        Some(name) => {
            dirs.with_write(async |dirs| {
                let list = IdentityList::load_from(dirs.read())?;
                change_default_identity(dirs, &list, name)?;
                println!("Set default identity to {name}");
                Ok(())
            })
            .await?
        }

        None => {
            let defaults = dirs
                .with_read(async |dirs| IdentityDefaults::load_from(dirs))
                .await??;
            println!("{}", defaults.default);
            Ok(())
        }
    }
}
