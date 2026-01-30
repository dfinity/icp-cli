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
    subcommand_help_heading = "Settings"
)]
enum Setting {
    /// Use Docker for the network launcher even when native mode is requested
    Autodockerize(AutodockerizeArgs),
}

#[derive(Debug, Args)]
struct AutodockerizeArgs {
    /// Set to true or false. If omitted, prints the current value.
    value: Option<bool>,
}

pub(crate) async fn exec(ctx: &Context, args: &SettingsArgs) -> Result<(), anyhow::Error> {
    match &args.setting {
        Setting::Autodockerize(sub_args) => exec_autodockerize(ctx, sub_args).await,
    }
}

async fn exec_autodockerize(ctx: &Context, args: &AutodockerizeArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.settings()?;

    match args.value {
        Some(value) => {
            dirs.with_write(async |dirs| {
                let mut settings = Settings::load_from(dirs.read())?;
                settings.autodockerize = value;
                settings.write_to(dirs)?;
                println!("Set autodockerize to {value}");
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
            println!("{}", settings.autodockerize);
            Ok(())
        }
    }
}
