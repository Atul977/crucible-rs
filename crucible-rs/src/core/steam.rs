// src/core/steam.rs — Steam store search + artwork download.

use std::path::PathBuf;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use crate::core::paths::{Paths, generic_dirs};

#[derive(Deserialize)] struct StoreSearch { items: Vec<StoreItem> }
#[derive(Deserialize)] struct StoreItem { id: u64, name: String }
#[derive(Deserialize)] struct AppDetails { success: bool, data: Option<AppData> }
#[derive(Deserialize)] struct AppData { header_image: Option<String>, name: Option<String> }

pub struct SteamApi {
    client: Client,
}

impl SteamApi {
    pub fn new() -> Self {
        Self { client: Client::builder().user_agent("Crucible/0.3").timeout(std::time::Duration::from_secs(10)).build().unwrap() }
    }

    /// Search the Steam store for a game; return (app_id, name) pairs.
    pub async fn search(&self, query: &str) -> Vec<(u64, String)> {
        let Ok(resp) = self.client.get("https://store.steampowered.com/api/storesearch/")
            .query(&[("term", query), ("l", "english"), ("cc", "US")])
            .send().await
        else { return vec![] };
        let Ok(s) = resp.json::<StoreSearch>().await else { return vec![] };
        s.items.into_iter().map(|i| (i.id, i.name)).take(10).collect()
    }

    /// Fetch the header image URL and canonical name for an app_id.
    pub async fn app_header(&self, app_id: u64) -> Option<(String, String)> {
        let url = format!("https://store.steampowered.com/api/appdetails?appids={app_id}");
        let resp = self.client.get(&url).send().await.ok()?;
        let text = resp.text().await.ok()?;
        let map: serde_json::Value = serde_json::from_str(&text).ok()?;
        let data = map.get(app_id.to_string())?.as_object()?;
        if !data.get("success")?.as_bool()? { return None; }
        let inner = data.get("data")?.as_object()?;
        let header = inner.get("header_image")?.as_str()?
            .split('?').next()?.to_string();
        let name = inner.get("name")?.as_str()?.to_string();
        Some((header, name))
    }

    /// Attempt to find the best matching Steam app for a game.
    /// Returns (app_id, steam_name) or None.
    pub async fn find_game(&self, exe_path: &str, game_name: &str) -> Option<(u64, String)> {
        let mut queries = Vec::new();

        // exe stem
        if let Some(stem) = std::path::Path::new(exe_path).file_stem().and_then(|s| s.to_str()) {
            queries.push(stem.replace('_'," ").replace('-'," "));
        }
        queries.push(game_name.to_string());

        // ancestor directories (non-generic)
        let mut cur = std::path::Path::new(exe_path).parent();
        let generic = generic_dirs();
        for _ in 0..8 {
            let Some(dir) = cur else { break };
            if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_lowercase();
                if !generic.contains(lower.as_str()) {
                    queries.push(name.replace(['_','-'], " "));
                }
            }
            cur = dir.parent();
        }

        let norm = |s: &str| {
            regex::Regex::new(r"[^a-z0-9 ]+").unwrap()
                .replace_all(&s.to_lowercase(), " ").trim().to_string()
        };
        let norm_game = norm(game_name);

        for q in queries.iter().take(4) {
            let candidates = self.search(q).await;
            for (id, cname) in candidates {
                let ncname = norm(&cname);
                let nq = norm(q);
                if ncname == norm_game || ncname == nq || ncname.contains(&nq) {
                    return Some((id, cname));
                }
            }
        }
        None
    }
}

// ── Artwork download ────────────────────────────────────────────────────────

pub async fn fetch_artwork(
    exe_path: &str,
    game_name: &str,
    api: &SteamApi,
) -> Option<()> {
    let key = artwork_key(exe_path);
    let art_dir = Paths::artwork_dir().join(&key);
    std::fs::create_dir_all(&art_dir).ok()?;

    let (app_id, _steam_name) = api.find_game(exe_path, game_name).await?;

    // Header image (used as portrait fallback)
    let (header_url, _) = api.app_header(app_id).await?;

    let client = reqwest::Client::builder().user_agent("Crucible/0.3").build().ok()?;

    // Download header
    let header_path = art_dir.join("header.jpg");
    if !header_path.exists() {
        if let Ok(resp) = client.get(&header_url).send().await {
            if let Ok(bytes) = resp.bytes().await {
                std::fs::write(&header_path, bytes).ok();
            }
        }
    }

    // Portrait (460x215 → try library capsule 600x900)
    let portrait_path = art_dir.join("portrait.jpg");
    if !portrait_path.exists() {
        let capsule = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/library_600x900_2x.jpg");
        if let Ok(resp) = client.get(&capsule).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    std::fs::write(&portrait_path, bytes).ok();
                }
            }
        }
        // fallback: hero
        if !portrait_path.exists() {
            let hero_url = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/library_hero.jpg");
            if let Ok(resp) = client.get(&hero_url).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        std::fs::write(&portrait_path, bytes).ok();
                    }
                }
            }
        }
    }

    Some(())
}

fn artwork_key(exe: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    if exe.is_empty() { return "unknown".into(); }
    let mut h = DefaultHasher::new();
    exe.to_lowercase().hash(&mut h);
    format!("exe_{:016x}", h.finish())
}
