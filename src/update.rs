use crossbeam_channel::Sender;
use crate::app::AppEvent;
use crate::config::Config;

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

    // Find the platform-appropriate installer asset
    let download_url = find_platform_asset(&resp).unwrap_or_default();

    if is_newer(remote_version, CURRENT_VERSION) {
        // Don't nag if the user already dismissed this exact version
        let config = Config::load();
        if config.update_dismissed_version == remote_version {
            return Ok(());
        }
        let _ = event_tx.try_send(AppEvent::UpdateAvailable {
            version: remote_version.to_string(),
            url: url.to_string(),
            download_url,
        });
    }

    Ok(())
}

/// Find the download URL for the current platform's installer asset.
fn find_platform_asset(release: &serde_json::Value) -> Option<String> {
    let assets = release["assets"].as_array()?;

    for asset in assets {
        let name = asset["name"].as_str().unwrap_or_default().to_lowercase();
        let url = asset["browser_download_url"].as_str().unwrap_or_default();

        #[cfg(windows)]
        {
            // Prefer the installer (PhonixSetup-*.exe) over the portable exe
            if name.starts_with("phonixsetup") && name.ends_with(".exe") {
                return Some(url.to_string());
            }
        }

        #[cfg(target_os = "macos")]
        {
            if name.ends_with(".dmg") {
                return Some(url.to_string());
            }
        }
    }

    None
}

/// Download the update installer in a background thread.
/// Sends UpdateDownloaded or UpdateFailed when done.
pub fn download_update(download_url: String, event_tx: Sender<AppEvent>) {
    std::thread::Builder::new()
        .name("phonix-update-download".into())
        .spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = event_tx.try_send(AppEvent::UpdateFailed(e.to_string()));
                    return;
                }
            };
            rt.block_on(async {
                match download_inner(&download_url).await {
                    Ok(path) => {
                        let _ = event_tx.try_send(AppEvent::UpdateDownloaded {
                            installer_path: path,
                        });
                    }
                    Err(e) => {
                        let _ = event_tx.try_send(AppEvent::UpdateFailed(e.to_string()));
                    }
                }
            });
        })
        .ok();
}

async fn download_inner(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client
        .get(url)
        .header("User-Agent", "phonix")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()).into());
    }

    // Determine filename from URL
    let filename = url
        .rsplit('/')
        .next()
        .unwrap_or("phonix-update");

    let temp_dir = std::env::temp_dir().join("phonix-update");
    std::fs::create_dir_all(&temp_dir)?;
    let dest = temp_dir.join(filename);

    let bytes = resp.bytes().await?;
    std::fs::write(&dest, &bytes)?;

    Ok(dest.to_string_lossy().to_string())
}

/// Launch the downloaded installer and exit the current process.
pub fn install_and_restart(installer_path: &str) {
    #[cfg(windows)]
    {
        use std::process::Command;
        // Launch the Inno Setup installer silently. The installer has
        // CloseApplications=force so it will close this process, install
        // the update, and relaunch Phonix automatically.
        let _ = Command::new(installer_path)
            .arg("/SILENT")
            .spawn();
        // Give the installer a moment to start, then exit so it can
        // replace the binary.
        std::thread::sleep(std::time::Duration::from_millis(1000));
        std::process::exit(0);
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // Open the DMG (mounts and shows in Finder)
        let _ = Command::new("open").arg(installer_path).spawn();
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = installer_path;
    }
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

/// Open a URL in the default browser.
pub fn open_in_browser(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(url)
            .spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(url)
            .spawn();
    }
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
