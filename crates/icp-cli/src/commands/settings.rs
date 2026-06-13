use std::{fmt, str::FromStr};

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
    /// Set the session length for password-protected PEM identities
    SessionLength(SessionLengthArgs),
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

#[derive(Debug, Args)]
struct SessionLengthArgs {
    /// Duration (e.g. `5m`, `1h`, `2d`) or `disabled`. If omitted, prints the current value.
    ///
    /// Note that due to clock drift, 2 minutes are added to the given value,
    /// so `5m` produces a 7-minute-expiry delegation. `disabled` turns off
    /// session caching entirely.
    value: Option<SessionLengthValue>,
}

/// A session-length value: a duration with suffix (`m`, `h`, `d`) or `disabled`.
#[derive(Debug, Clone)]
pub struct SessionLengthValue(pub Option<u32>);

impl FromStr for SessionLengthValue {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "disabled" {
            return Ok(Self(None));
        }
        let (digits, unit_secs) = if let Some(d) = s.strip_suffix('m') {
            (d, 60u64)
        } else if let Some(d) = s.strip_suffix('h') {
            (d, 3600)
        } else if let Some(d) = s.strip_suffix('d') {
            (d, 86400)
        } else {
            return Err(format!(
                "expected a duration like `5m`, `1h`, `2d`, or `disabled`; got `{s}`"
            ));
        };
        let n: u64 = digits
            .parse()
            .map_err(|_| format!("expected a whole number before the suffix, got `{digits}`"))?;
        let total_secs = n
            .checked_mul(unit_secs)
            .ok_or_else(|| "duration too large".to_string())?;
        // Round up to whole minutes.
        let minutes = total_secs.div_ceil(60);
        let minutes = u32::try_from(minutes).map_err(|_| "duration too large".to_string())?;
        Ok(Self(Some(minutes)))
    }
}

impl fmt::Display for SessionLengthValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(n) => write!(f, "{n}m"),
            None => write!(f, "disabled"),
        }
    }
}

pub(crate) async fn exec(ctx: &Context, args: &SettingsArgs) -> Result<(), anyhow::Error> {
    match &args.setting {
        Setting::Autocontainerize(sub_args) => exec_autocontainerize(ctx, sub_args).await,
        Setting::Telemetry(sub_args) => exec_telemetry(ctx, sub_args).await,
        Setting::UpdateCheck(sub_args) => exec_update_check(ctx, sub_args).await,
        Setting::SessionLength(sub_args) => exec_session_length(ctx, sub_args).await,
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

async fn exec_session_length(ctx: &Context, args: &SessionLengthArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.settings()?;

    match &args.value {
        Some(SessionLengthValue(value)) => {
            let value = *value;
            dirs.with_write(async |dirs| {
                let mut settings = Settings::load_from(dirs.read())?;
                settings.session_length = value;
                settings.write_to(dirs)?;
                info!("Set session-length to {}", SessionLengthValue(value));
                Ok(())
            })
            .await?
        }

        None => {
            let settings = dirs
                .with_read(async |dirs| Settings::load_from(dirs))
                .await??;
            println!("{}", SessionLengthValue(settings.session_length));
            Ok(())
        }
    }
}
