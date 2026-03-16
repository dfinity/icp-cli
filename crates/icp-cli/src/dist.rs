use std::sync::LazyLock;

use axoupdater::AxoUpdater;
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
        .filter(|t| newer_than_current(t.strip_prefix(prefix).unwrap_or(t)))
        .map(|v| v.to_string()))
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
