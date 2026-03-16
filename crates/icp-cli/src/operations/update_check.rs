use std::time::{Duration, SystemTime};

use icp::settings::UpdateCheck;
use tracing::debug;

use crate::dist::{dist_check_for_updates, dist_supports_update_check};

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
    let _ = icp::fs::write(&nag_path, format!("{now}\n").as_bytes());

    let client = reqwest::Client::new();
    dist_check_for_updates(&client, beta).await
}
