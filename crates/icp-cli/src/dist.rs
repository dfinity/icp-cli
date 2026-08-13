use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use axoupdater::AxoUpdater;
use icp::settings::UpdateCheck;
use reqwest::Client;
use tracing::debug;

enum DistChannel {
    HomebrewCore,
    HomebrewBeta,
    Npm,
    AxoDist,
    Custom,
}

static DIST_CHANNEL: LazyLock<DistChannel> = LazyLock::new(|| {
    let rt;
    let mut var = option_env!("ICP_CLI_BUILD_DIST");
    if var.is_none() {
        rt = std::env::var("ICP_CLI_DIST").ok();
        var = rt.as_deref();
    }
    match var {
        Some("homebrew-core") => DistChannel::HomebrewCore,
        Some("homebrew-beta") => DistChannel::HomebrewBeta,
        Some("npm") => DistChannel::Npm,
        Some("dist") => DistChannel::AxoDist, // envvar is not used for dist currently, this is just for testing
        Some(_) => DistChannel::Custom,
        None => {
            let mut updater = AxoUpdater::new_for("icp-cli");
            let Ok(_) = updater.load_receipt() else {
                return DistChannel::Custom;
            };
            if updater
                .check_receipt_is_for_this_executable()
                .unwrap_or(false)
            {
                DistChannel::AxoDist
            } else {
                DistChannel::Custom
            }
        }
    }
});

pub fn dist_update_suggestion(ver: &str) -> Option<&'static str> {
    let is_beta = ver.contains("-beta.");
    match *DIST_CHANNEL {
        DistChannel::HomebrewCore => Some("Run `brew upgrade icp-cli` to update"),
        DistChannel::HomebrewBeta => Some("Run `brew upgrade icp-cli-beta` to update"),
        DistChannel::Npm => {
            if is_beta {
                Some("Run `npm install -g @icp-sdk/icp-cli@beta` to update")
            } else {
                Some("Run `npm install -g @icp-sdk/icp-cli` to update")
            }
        }
        DistChannel::AxoDist => {
            if is_beta {
                Some("Run `icp-cli-update --prerelease` to update")
            } else {
                Some("Run `icp-cli-update` to update")
            }
        }
        DistChannel::Custom => None,
    }
}

pub fn dist_supports_betas() -> bool {
    matches!(*DIST_CHANNEL, DistChannel::AxoDist | DistChannel::Npm)
}

pub fn dist_supports_update_check() -> bool {
    !matches!(*DIST_CHANNEL, DistChannel::Custom)
}

/// Check whether a newer version is available via the distribution channel.
/// Returns `Some(latest_version)` if an update is available, `None` otherwise.
pub async fn dist_check_for_updates(client: &Client, beta_setting: bool) -> Option<String> {
    let result = match *DIST_CHANNEL {
        DistChannel::Custom => return None,
        DistChannel::AxoDist => check_github(client, "icp-cli", "v", beta_setting).await,
        DistChannel::HomebrewBeta => {
            // betas are marked as full releases in the tap
            check_github(client, "homebrew-tap", "icp-cli-beta-", false).await
        }
        DistChannel::HomebrewCore => check_homebrew(client, "icp-cli").await,
        DistChannel::Npm => check_npm(client, beta_setting).await,
    };
    match result {
        Ok(v) => v,
        Err(e) => {
            debug!("Update check failed: {e}");
            None
        }
    }
}

async fn check_github(
    client: &Client,
    repo: &str,
    prefix: &str,
    include_prereleases: bool,
) -> reqwest::Result<Option<String>> {
    let url = format!("https://api.github.com/repos/dfinity/{repo}/releases");
    let mut req = client.get(url).header("User-Agent", "icp-cli");
    if let Ok(token) = std::env::var("ICP_CLI_GITHUB_TOKEN") {
        req = req.bearer_auth(token);
    }

    let response: serde_json::Value = req.send().await?.error_for_status()?.json().await?;

    let tag = response
        .as_array()
        .and_then(|releases| {
            releases.iter().find(|r| {
                !r["draft"].as_bool().unwrap_or(false)
                    && r["tag_name"]
                        .as_str()
                        .is_some_and(|t| t.starts_with(prefix))
                    && (include_prereleases || !r["prerelease"].as_bool().unwrap_or(false))
            })
        })
        .and_then(|r| r["tag_name"].as_str());
    Ok(tag
        .map(|t| t.strip_prefix(prefix).unwrap_or(t).to_string())
        .filter(|t| newer_than_current(t)))
}

async fn check_homebrew(client: &Client, formula: &str) -> reqwest::Result<Option<String>> {
    let url = format!("https://formulae.brew.sh/api/formula/{formula}.json");
    let response: serde_json::Value = client
        .get(&url)
        .header("User-Agent", "icp-cli")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let version = response["versions"]["stable"].as_str();
    Ok(version
        .filter(|s| newer_than_current(s))
        .map(|v| v.to_string()))
}

async fn check_npm(client: &Client, beta: bool) -> reqwest::Result<Option<String>> {
    let url = "https://registry.npmjs.org/@icp-sdk/icp-cli";
    let response: serde_json::Value = client
        .get(url)
        .header("User-Agent", "icp-cli")
        .header("Accept", "application/vnd.npm.install-v1+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let version = if beta {
        response["dist-tags"]["beta"].as_str()
    } else {
        response["dist-tags"]["latest"].as_str()
    };
    Ok(version
        .filter(|s| newer_than_current(s))
        .map(|v| v.to_string()))
}

fn newer_than_current(version_str: &str) -> bool {
    let Ok(current) = semver::Version::parse(env!("CARGO_PKG_VERSION")) else {
        return false;
    };
    let clean = version_str.strip_prefix('v').unwrap_or(version_str);
    let Ok(latest) = semver::Version::parse(clean) else {
        return false;
    };
    latest > current
}

const ONE_DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Check for CLI updates, returning the latest version string if one is available.
pub(crate) async fn update_check(ctx: &icp::context::Context) -> Option<String> {
    let update_check_setting = match ctx.dirs.settings() {
        Ok(dirs) => {
            dirs.with_read(async |dirs| icp::settings::Settings::load_from(dirs).ok())
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
                .update_check
        }
        Err(_) => UpdateCheck::Releases,
    };

    let enabled =
        !matches!(update_check_setting, UpdateCheck::Disabled) && dist_supports_update_check();
    if !enabled {
        return None;
    }
    let beta = matches!(update_check_setting, UpdateCheck::Betas);
    let nag_path = ctx.dirs.cli_update_nag_timestamp();

    // Throttle to at most once per day
    if let Ok(contents) = icp::fs::read_to_string(&nag_path)
        && let Ok(ts) = contents.trim().parse::<u64>()
    {
        let then = SystemTime::UNIX_EPOCH + Duration::from_secs(ts);
        if then.elapsed().unwrap_or(Duration::ZERO) < ONE_DAY {
            debug!("Skipping CLI update check (last check < 24h ago)");
            return None;
        }
    }

    // update the timestamp regardless of result
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("since epoch")
        .as_secs();
    let _ = icp::fs::create_dir_all(nag_path.parent().unwrap());
    let _ = icp::fs::write(&nag_path, format!("{now}\n").as_bytes());

    let client = reqwest::Client::new();
    dist_check_for_updates(&client, beta).await
}
