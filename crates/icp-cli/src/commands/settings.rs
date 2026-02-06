use clap::{Args, Subcommand};
use icp::{context::Context, settings::Settings};

#[derive(Debug, Args)]
pub(crate) struct SettingsArgs {
    #[command(subcommand)]
    setting: Setting,
}

#[derive(Debug, Subcommand)]
#[command(
    subcommand_value_name = "SETTING",
    subcommand_help_heading = "Settings",
    override_usage = "icp settings [OPTIONS] <SETTING> [VALUE]",
    disable_help_subcommand = true
)]
enum Setting {
    /// Use Docker for the network launcher even when native mode is requested
    Autocontainerize(AutocontainerizeArgs),
}

#[derive(Debug, Args)]
struct AutocontainerizeArgs {
    /// Set to true or false. If omitted, prints the current value.
    value: Option<bool>,
}

pub(crate) async fn exec(ctx: &Context, args: &SettingsArgs) -> Result<(), anyhow::Error> {
    match &args.setting {
        Setting::Autocontainerize(sub_args) => exec_autocontainerize(ctx, sub_args).await,
    }
}

async fn exec_autocontainerize(
    ctx: &Context,
    args: &AutocontainerizeArgs,
) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.settings()?;

    match args.value {
        Some(value) => {
            dirs.with_write(async |dirs| {
                let mut settings = Settings::load_from(dirs.read())?;
                settings.autocontainerize = value;
                settings.write_to(dirs)?;
                println!("Set autocontainerize to {value}");
                if cfg!(windows) {
                    eprintln!(
                        "Warning: This setting is ignored on Windows. \
                        Docker is always used because the network launcher does not run natively."
                    );
                }
                Ok(())
            })
            .await?
        }

        None => {
            let settings = dirs
                .with_read(async |dirs| Settings::load_from(dirs))
                .await??;
            println!("{}", settings.autocontainerize);
            Ok(())
        }
    }
}
