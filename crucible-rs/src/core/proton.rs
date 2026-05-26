// src/core/proton.rs — Download and install Proton-GE / UMU-Proton / CachyOS.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct ProtonSource {
    pub id: &'static str,
    pub repo: &'static str,
    pub suffix: &'static str,
    pub label: &'static str,
}

pub const SOURCES: &[ProtonSource] = &[
    ProtonSource { id: "ge",    repo: "GloriousEggroll/proton-ge-custom",  suffix: ".tar.gz",       label: "GE-Proton" },
    ProtonSource { id: "umu",   repo: "Open-Wine-Components/umu-proton",   suffix: ".tar.gz",       label: "UMU-Proton" },
    ProtonSource { id: "cachy", repo: "CachyOS/proton-cachyos",            suffix: "-x86_64.tar.xz", label: "CachyOS" },
];

#[derive(Debug, Clone)]
pub struct ProtonRelease {
    pub tag: String,
    pub url: String,
    pub size: u64,
    pub source: String,
    pub installed: bool,
}

// GitHub release JSON shapes
#[derive(Deserialize)] struct GhRelease { tag_name: String, assets: Vec<GhAsset> }
#[derive(Deserialize)] struct GhAsset { name: String, browser_download_url: String, size: Option<u64> }

fn compat_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".steam/steam/compatibilitytools.d")
}

fn installed_tags(extra_dirs: &[PathBuf]) -> Vec<String> {
    let mut tags = Vec::new();
    let default = compat_dir();
    for dir in std::iter::once(&default).chain(extra_dirs.iter()) {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("proton").exists() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    tags.push(name.to_string());
                }
            }
        }
    }
    tags
}

pub async fn fetch_releases(source_id: &str, extra_dirs: &[PathBuf]) -> Result<Vec<ProtonRelease>> {
    let src = SOURCES.iter().find(|s| s.id == source_id)
        .ok_or_else(|| anyhow::anyhow!("unknown source: {source_id}"))?;

    let client = reqwest::Client::builder().user_agent("Crucible/0.3").build()?;
    let url = format!("https://api.github.com/repos/{}/releases?per_page=20", src.repo);
    let releases: Vec<GhRelease> = client.get(&url).send().await?.error_for_status()?.json().await?;

    let installed = installed_tags(extra_dirs);
    let mut result = Vec::new();

    for rel in releases {
        let Some(asset) = rel.assets.iter()
            .find(|a| a.name.ends_with(src.suffix) && !a.name.ends_with(".sha512sum"))
        else { continue };

        result.push(ProtonRelease {
            tag: rel.tag_name.clone(),
            url: asset.browser_download_url.clone(),
            size: asset.size.unwrap_or(0),
            source: source_id.to_string(),
            installed: installed.iter().any(|t| t == &rel.tag_name),
        });
    }
    Ok(result)
}

pub async fn download_and_install(
    release: ProtonRelease,
    extra_dirs: &[PathBuf],
    progress: impl Fn(f32, &str) + Send + 'static,
) -> Result<()> {
    let dest_dir = compat_dir().join(&release.tag);
    if dest_dir.exists() && dest_dir.join("proton").exists() {
        progress(1.0, "Already installed");
        return Ok(());
    }

    let _ = std::fs::create_dir_all(compat_dir());
    let suffix = if release.url.ends_with(".tar.xz") { ".tar.xz" } else { ".tar.gz" };
    let tmp_archive = std::env::temp_dir().join(format!("crucible-proton-{}{suffix}", release.tag));

    // ── Download ────────────────────────────────────────────────────────────
    let client = reqwest::Client::builder().user_agent("Crucible/0.3").build()?;
    let resp = client.get(&release.url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(&tmp_archive).await
        .context("creating temp archive")?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download error")?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            progress(downloaded as f32 / total as f32 * 0.7, "Downloading…");
        }
    }
    file.flush().await?;
    drop(file);

    progress(0.7, "Extracting…");

    // ── Extract (blocking) ──────────────────────────────────────────────────
    let tag = release.tag.clone();
    let archive_path = tmp_archive.clone();
    let dest_parent = compat_dir();

    tokio::task::spawn_blocking(move || -> Result<()> {
        let tmp_extract = dest_parent.join(format!(".{tag}.installing"));
        let _ = std::fs::remove_dir_all(&tmp_extract);
        std::fs::create_dir_all(&tmp_extract)?;

        let f = std::fs::File::open(&archive_path)?;
        if archive_path.to_string_lossy().ends_with(".tar.xz") {
            let dec = xz2::read::XzDecoder::new(f);
            extract_tar(dec, &tmp_extract, &tag)?;
        } else {
            let dec = flate2::read::GzDecoder::new(f);
            extract_tar(dec, &tmp_extract, &tag)?;
        }

        let final_dir = dest_parent.join(&tag);
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(&tmp_extract, &final_dir)?;
        std::fs::remove_file(&archive_path).ok();
        Ok(())
    })
    .await
    .context("extraction task panicked")?
    .context("extraction failed")?;

    progress(1.0, "Installed");
    Ok(())
}

fn extract_tar<R: std::io::Read>(r: R, dest: &PathBuf, tag: &str) -> Result<()> {
    let mut archive = tar::Archive::new(r);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        // Strip the leading top-level directory (e.g. GE-Proton9-27/)
        let stripped: PathBuf = path.components().skip(1).collect();
        if stripped.as_os_str().is_empty() { continue; }
        // Path traversal guard
        let out = dest.join(&stripped);
        if !out.starts_with(dest) {
            bail!("Archive member escapes destination: {}", path.display());
        }
        if let Some(parent) = out.parent() { std::fs::create_dir_all(parent)?; }
        entry.unpack(&out)?;
    }
    Ok(())
}

pub fn delete_version(tag: &str) -> Result<()> {
    let dir = compat_dir().join(tag);
    if !dir.exists() { bail!("Proton version '{tag}' not installed"); }
    std::fs::remove_dir_all(&dir)
        .with_context(|| format!("deleting {}", dir.display()))
}
