use std::time::Duration;

use clap::Args;
use icp::{
    context::Context,
    identity::{
        key,
        manifest::{IdentityList, IdentitySpec, PemFormat},
    },
    settings::Settings,
};
use snafu::{OptionExt, ResultExt, Snafu};
use tracing::info;

use crate::commands::identity::{delegation::sign::DurationArg, link::ii};

/// Re-authenticate an Internet Identity delegation or create a PEM session delegation
#[derive(Debug, Args)]
pub(crate) struct LoginArgs {
    /// Name of the identity to re-authenticate
    name: String,

    /// Session delegation duration (e.g. "30m", "8h", "1d").
    /// Required for PEM identities when session caching is disabled in settings.
    /// Not applicable for Internet Identity.
    #[arg(long)]
    duration: Option<DurationArg>,
}

pub(crate) async fn exec(ctx: &Context, args: &LoginArgs) -> Result<(), LoginError> {
    let spec = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            let list = IdentityList::load_from(dirs)?;
            list.identities
                .get(&args.name)
                .cloned()
                .context(IdentityNotFoundSnafu { name: &args.name })
        })
        .await??;

    match spec {
        IdentitySpec::InternetIdentity {
            algorithm,
            storage,
            host,
            ..
        } => {
            if args.duration.is_some() {
                return DurationSnafu { name: &args.name }.fail();
            }

            let password_func = ctx.password_func.clone();
            let der_public_key = ctx
                .dirs
                .identity()?
                .with_read(async |dirs| {
                    key::load_ii_session_public_key(dirs, &args.name, &algorithm, &storage, || {
                        password_func()
                    })
                })
                .await?
                .context(LoadSessionKeySnafu)?;

            let chain = ii::recv_delegation(&host, &der_public_key)
                .await
                .context(PollSnafu)?;

            ctx.dirs
                .identity()?
                .with_write(async |dirs| key::update_ii_delegation(dirs, &args.name, &chain))
                .await?
                .context(UpdateDelegationSnafu)?;

            info!("Identity `{}` re-authenticated", args.name);
        }

        IdentitySpec::Pem {
            format: PemFormat::Pbes2,
            algorithm,
            ..
        } => {
            let duration = match &args.duration {
                Some(d) => Duration::from_nanos(d.as_nanos()) + Duration::from_secs(5 * 60),
                None => {
                    let settings = ctx
                        .dirs
                        .settings()?
                        .with_read(async |dirs| Settings::load_from(dirs))
                        .await??;
                    settings
                        .session_length
                        .map(|m| Duration::from_secs(u64::from(m + 5) * 60))
                        .context(DurationRequiredSnafu { name: &args.name })?
                }
            };

            let password_func = ctx.password_func.clone();
            ctx.dirs
                .identity()?
                .with_read(async |dirs| {
                    key::create_explicit_pem_session(
                        dirs,
                        &args.name,
                        &algorithm,
                        || password_func(),
                        duration,
                    )
                })
                .await?
                .context(CreatePemSessionSnafu)?;

            info!("Session delegation created for identity `{}`", args.name);
        }
        _ => {
            return UnsupportedIdentityTypeSnafu { name: &args.name }.fail();
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum LoginError {
    #[snafu(transparent)]
    LockDir { source: icp::fs::lock::LockError },

    #[snafu(transparent)]
    LoadManifest {
        source: icp::identity::manifest::LoadIdentityManifestError,
    },

    #[snafu(transparent)]
    LoadSettings {
        source: icp::settings::LoadSettingsError,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    IdentityNotFound { name: String },

    #[snafu(display("`--duration` cannot be used with Internet Identity `{name}`"))]
    Duration { name: String },

    #[snafu(display(
        "session caching is disabled; specify `--duration` to create a session delegation for `{name}`"
    ))]
    DurationRequired { name: String },

    #[snafu(display("identity `{name}` does not support logins"))]
    UnsupportedIdentityType { name: String },

    #[snafu(display("failed to load II session key"))]
    LoadSessionKey { source: key::LoadIdentityError },

    #[snafu(display("failed during II authentication"))]
    Poll { source: ii::IiRecvError },

    #[snafu(display("failed to update delegation"))]
    UpdateDelegation {
        source: key::UpdateIiDelegationError,
    },

    #[snafu(display("failed to create PEM session delegation"))]
    CreatePemSession {
        source: key::CreateExplicitPemSessionError,
    },
}
