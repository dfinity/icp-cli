use clap::{Args, Subcommand};
use icp::{
    context::Context,
    settings::{Settings, UpdateCheck},
};
use tracing::{info, warn};

use crate::dist::dist_supports_betas;

/// Configure user settings
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
    /// Enable or disable anonymous usage telemetry
    Telemetry(TelemetryArgs),
    /// Enable or disable the CLI update check
    UpdateCheck(UpdateCheckArgs),
}

#[derive(Debug, Args)]
struct AutocontainerizeArgs {
    /// Set to true or false. If omitted, prints the current value.
    value: Option<bool>,
}

#[derive(Debug, Args)]
struct TelemetryArgs {
    /// Set to true or false. If omitted, prints the current value.
    value: Option<bool>,
}

#[derive(Debug, Args)]
struct UpdateCheckArgs {
    /// Set to releases, betas, or disabled. If omitted, prints the current value.
    #[arg(value_enum)]
    value: Option<UpdateCheck>,
}

pub(crate) async fn exec(ctx: &Context, args: &SettingsArgs) -> Result<(), anyhow::Error> {
    match &args.setting {
        Setting::Autocontainerize(sub_args) => exec_autocontainerize(ctx, sub_args).await,
        Setting::Telemetry(sub_args) => exec_telemetry(ctx, sub_args).await,
        Setting::UpdateCheck(sub_args) => exec_update_check(ctx, sub_args).await,
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
                info!("Set autocontainerize to {value}");
                if cfg!(windows) {
                    warn!(
                        "This setting is ignored on Windows. \
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

async fn exec_telemetry(ctx: &Context, args: &TelemetryArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.settings()?;

    match args.value {
        Some(value) => {
            dirs.with_write(async |dirs| {
                let mut settings = Settings::load_from(dirs.read())?;
                settings.telemetry_enabled = value;
                settings.write_to(dirs)?;
                info!("Set telemetry to {value}");
                Ok(())
            })
            .await?
        }

        None => {
            let settings = dirs
                .with_read(async |dirs| Settings::load_from(dirs))
                .await??;
            println!("{}", settings.telemetry_enabled);
            Ok(())
        }
    }
}

async fn exec_update_check(ctx: &Context, args: &UpdateCheckArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.settings()?;

    match args.value {
        Some(value) => {
            if value == UpdateCheck::Betas && !dist_supports_betas() {
                warn!("The 'betas' setting has no effect for this distribution channel.");
            }
            dirs.with_write(async |dirs| {
                let mut settings = Settings::load_from(dirs.read())?;
                settings.update_check = value;
                settings.write_to(dirs)?;
                info!("Set update-check to {value}");
                Ok(())
            })
            .await?
        }

        None => {
            let settings = dirs
                .with_read(async |dirs| Settings::load_from(dirs))
                .await??;
            println!("{}", settings.update_check);
            Ok(())
        }
    }
}
