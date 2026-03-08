use crossbeam_channel::Sender;
use crate::app::AppEvent;

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/mrchevyceleb/phonix/releases/latest";

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Spawn a background thread that checks GitHub for a newer release.
/// If one is found, sends `AppEvent::UpdateAvailable` on the channel.
/// Silently does nothing on any error.
pub fn check_for_updates(event_tx: Sender<AppEvent>) {
    std::thread::Builder::new()
        .name("phonix-update-check".into())
        .spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(_) => return,
            };
            rt.block_on(async {
                let _ = check_inner(&event_tx).await;
            });
        })
        .ok();
}

async fn check_inner(event_tx: &Sender<AppEvent>) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(GITHUB_RELEASES_URL)
        .header("User-Agent", "phonix")
        .send()
        .await?
        .json()
        .await?;

    let tag = resp["tag_name"].as_str().unwrap_or_default();
    let remote_version = tag.strip_prefix('v').unwrap_or(tag);

    // Skip pre-release tags (e.g. "1.0.1-beta")
    if remote_version.contains('-') {
        return Ok(());
    }

    let url = resp["html_url"].as_str().unwrap_or_default();

    if is_newer(remote_version, CURRENT_VERSION) {
        let _ = event_tx.try_send(AppEvent::UpdateAvailable {
            version: remote_version.to_string(),
            url: url.to_string(),
        });
    }

    Ok(())
}

/// Returns true if `remote` is a higher semver than `local`.
fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let mut parts = s.splitn(3, '.');
        let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| {
            p.split('-').next().and_then(|n| n.parse().ok())
        }).unwrap_or(0);
        (major, minor, patch)
    };
    parse(remote) > parse(local)
}

/// Open a URL in the default browser (Windows).
pub fn open_in_browser(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("1.1.0", "1.0.0"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(is_newer("1.1.0", "1.0.0-beta"));
    }
}
