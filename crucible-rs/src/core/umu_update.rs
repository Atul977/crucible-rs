// src/core/umu_update.rs — Silently update umu-run from GitHub on startup.

use anyhow::{Context, Result};
use crate::core::paths::Paths;

const API: &str =
    "https://api.github.com/repos/Open-Wine-Components/umu-launcher/releases/latest";
const SUFFIX: &str = "-zipapp.tar";
const ALLOWED_HOSTS: &[&str] = &["github.com","objects.githubusercontent.com",
                                   "release-assets.githubusercontent.com"];

pub async fn update_umu() -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("Crucible/0.3")
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    let data: serde_json::Value = client.get(API).send().await?.error_for_status()?.json().await?;
    let tag = data["tag_name"].as_str().unwrap_or("").to_string();
    if tag.is_empty() { return Ok(()); }

    let runner_dir = Paths::runner_dir();
    let dest = runner_dir.join("umu-run");
    let ver_file = runner_dir.join("umu-run.version");
    let current = std::fs::read_to_string(&ver_file).unwrap_or_default();

    if current.trim() == tag && dest.is_file() {
        log::debug!("umu-run {tag} is current");
        return Ok(());
    }

    // Find the asset
    let assets = data["assets"].as_array().context("no assets")?;
    let url = assets.iter()
        .find_map(|a| {
            let name = a["name"].as_str().unwrap_or("");
            if name.ends_with(SUFFIX) {
                a["browser_download_url"].as_str().map(|s| s.to_string())
            } else { None }
        })
        .context("no zipapp asset found")?;

    // Validate URL host
    let parsed = url::Url::parse(&url)?;
    if parsed.scheme() != "https" || !ALLOWED_HOSTS.contains(&parsed.host_str().unwrap_or("")) {
        anyhow::bail!("untrusted download URL: {url}");
    }

    log::info!("Updating umu-run to {tag}…");

    let tmp = std::env::temp_dir().join(format!("crucible-umu-run-{tag}.tar"));
    let resp = client.get(&url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    std::fs::write(&tmp, &bytes)?;

    // Extract umu-run from the tar
    let extracted_bin = tokio::task::spawn_blocking({
        let tmp = tmp.clone();
        let dest = dest.clone();
        move || -> Result<()> {
            let f = std::fs::File::open(&tmp)?;
            let mut archive = tar::Archive::new(f);
            let runner_dir = dest.parent().unwrap();
            let _ = std::fs::create_dir_all(runner_dir);
            for entry in archive.entries()? {
                let mut e = entry?;
                let path = e.path()?.into_owned();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "umu-run" {
                    let out = runner_dir.join("umu-run.new");
                    e.unpack(&out)?;
                    #[cfg(unix)] {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&out,
                            std::fs::Permissions::from_mode(0o755));
                    }
                    std::fs::rename(&out, &dest)?;
                    return Ok(());
                }
            }
            anyhow::bail!("umu-run not found in archive")
        }
    }).await.context("extract task panicked")??;

    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&ver_file, &tag)?;
    log::info!("umu-run updated to {tag}");
    Ok(())
}
