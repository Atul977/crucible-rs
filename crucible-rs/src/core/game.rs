// src/core/game.rs — Per-game config and game library manager.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use crate::core::config::{GlobalConfig, GamescopeSettings};
use crate::core::paths::{Paths, safe_name, display_name_from_exe, find_game_root};

// ── GameConfig ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameConfig {
    pub name: String,
    #[serde(default)] pub exe_path: String,
    #[serde(default)] pub proton_version: String,
    #[serde(default)] pub proton_path: String,
    #[serde(default)] pub launch_args: String,
    #[serde(default)] pub custom_overrides: String,
    #[serde(default)] pub install_dir: String,
    #[serde(default)] pub env_vars: HashMap<String, String>,
    #[serde(default)] pub disabled_env_vars: Vec<String>,
    #[serde(default)] pub prefix_path: String,
    #[serde(default)] pub fingerprint_lock: bool,
    #[serde(default)] pub wrapper_command: String,
    #[serde(default = "auto_str")] pub exe_match_mode: String,
    #[serde(default)] pub enable_gamemode: bool,
    #[serde(default)] pub enable_mangohud: bool,
    #[serde(default)] pub enable_gamescope: bool,
    #[serde(default)] pub disabled_global_flags: Vec<String>,
    #[serde(default)] pub gamescope_settings: GamescopeSettings,
    #[serde(default)] pub playtime_seconds: u64,
    #[serde(default)] pub last_played: String,

    #[serde(skip)]
    pub game_file: String,
}
fn auto_str() -> String { "auto".into() }

impl GameConfig {
    pub fn playtime_display(&self) -> String {
        let s = self.playtime_seconds;
        if s >= 3600      { format!("{}h {}m", s / 3600, (s % 3600) / 60) }
        else if s >= 60   { format!("{}m", s / 60) }
        else if s > 0     { format!("{}s", s) }
        else              { String::new() }
    }

    /// Artwork directory key (stable hash of exe path).
    pub fn artwork_key(&self) -> String {
        artwork_key_for(&self.exe_path)
    }

    pub fn cover_path(&self) -> PathBuf {
        Paths::artwork_dir().join(self.artwork_key()).join("portrait.jpg")
    }
    pub fn hero_path(&self) -> PathBuf {
        Paths::artwork_dir().join(self.artwork_key()).join("hero.jpg")
    }
    pub fn header_path(&self) -> PathBuf {
        Paths::artwork_dir().join(self.artwork_key()).join("header.jpg")
    }
}

fn artwork_key_for(exe: &str) -> String {
    if exe.is_empty() { return "unknown".into(); }
    // Use a stable 16-char hex key based on the lowercased exe path.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    exe.to_lowercase().hash(&mut h);
    format!("exe_{:016x}", h.finish())
}

// ── DLL override builder ──────────────────────────────────────────────────

pub fn build_dll_overrides(custom: &str) -> String {
    let aliases = [("native","n"),("builtin","b"),("disabled","d")];
    let valid = ["n,b","b,n","b","n","d"];
    let mut buckets: HashMap<&str, Vec<String>> = valid.iter().map(|k| (*k, vec![])).collect();
    buckets.get_mut("d").unwrap().push("winemenubuilder".into());

    if !custom.is_empty() {
        if custom.contains('=') {
            for part in custom.split(';') {
                let part = part.trim();
                if part.is_empty() { continue; }
                if let Some((dll, mode_raw)) = part.split_once('=') {
                    let dll = dll.trim();
                    let mut mode = mode_raw.trim().to_lowercase();
                    for (alias, replacement) in &aliases {
                        if mode == *alias { mode = replacement.to_string(); break; }
                    }
                    if dll.is_empty() { continue; }
                    let mode = if valid.contains(&mode.as_str()) { mode } else { "n,b".to_string() };
                    buckets.get_mut(mode.as_str()).unwrap().push(dll.to_string());
                } else {
                    let dll = part.trim().to_string();
                    if !dll.is_empty() { buckets.get_mut("n,b").unwrap().push(dll); }
                }
            }
        } else {
            for dll in custom.split(',') {
                let dll = dll.trim().to_string();
                if !dll.is_empty() { buckets.get_mut("n,b").unwrap().push(dll); }
            }
        }
    }

    valid.iter()
        .filter_map(|mode| {
            let dlls = buckets.get(*mode).unwrap();
            if dlls.is_empty() { None }
            else {
                let mut sorted = dlls.clone();
                sorted.sort();
                Some(format!("{}={}", sorted.join(","), mode))
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

// ── ProtonManager (scan installed) ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InstalledProton {
    pub name: String,
    pub path: String,
    pub version: String,
    pub source: String,   // "ge" | "umu" | "cachy"
}

// ── GameManager ───────────────────────────────────────────────────────────

pub struct GameManager {
    pub games: Vec<GameConfig>,
    pub global_config: GlobalConfig,
}

impl GameManager {
    pub fn new() -> Self {
        let mut gm = Self { games: vec![], global_config: GlobalConfig::load() };
        gm.scan_games();
        gm
    }

    pub fn scan_games(&mut self) {
        self.games.clear();
        let dir = Paths::games_dir();
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter().flatten().flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
            .collect();
        paths.sort();
        for p in paths {
            match Self::load_file(&p) {
                Ok(mut g) => {
                    if g.install_dir.is_empty() {
                        g.install_dir = find_game_root(&g.exe_path)
                            .or_else(|| Path::new(&g.exe_path).parent()
                                .map(|p| p.to_string_lossy().into_owned()))
                            .unwrap_or_default();
                    }
                    self.games.push(g);
                }
                Err(e) => log::warn!("skip {}: {e}", p.display()),
            }
        }
    }

    fn load_file(path: &Path) -> Result<GameConfig> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        let mut g: GameConfig = serde_json::from_str(&text)
            .with_context(|| format!("parse {}", path.display()))?;
        g.game_file = path.to_string_lossy().into_owned();
        Ok(g)
    }

    fn write_file(path: &Path, g: &GameConfig) -> Result<()> {
        // Atomic write via temp file
        let dir = path.parent().unwrap_or(path);
        let tmp = dir.join(format!(".{}.tmp", path.file_name().unwrap().to_string_lossy()));
        let text = serde_json::to_string_pretty(g)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn games_sorted(&self) -> Vec<&GameConfig> {
        let mut v: Vec<&GameConfig> = self.games.iter().collect();
        v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        v
    }

    pub fn get(&self, name: &str) -> Option<&GameConfig> {
        self.games.iter().find(|g| g.name == name)
    }

    pub fn add_game(
        &mut self,
        name: String, exe_path: String, proton_version: String,
        launch_args: String, custom_overrides: String, install_dir: String,
        env_vars: HashMap<String, String>, prefix_path: String,
        fingerprint_lock: bool, wrapper_command: String, exe_match_mode: String,
        enable_gamemode: bool, enable_mangohud: bool, enable_gamescope: bool,
        gamescope_settings: GamescopeSettings,
        extra_proton_dirs: &[PathBuf],
    ) -> Result<GameConfig> {
        let proton_path = self.find_proton_in_dirs(&proton_version, extra_proton_dirs)
            .unwrap_or_default();
        let games_dir = Paths::games_dir();
        let file_path = games_dir.join(format!("{}.json", safe_name(&name)));

        let existing = self.get(&name).cloned();
        let playtime_seconds = existing.as_ref().map(|g| g.playtime_seconds).unwrap_or(0);
        let last_played      = existing.as_ref().map(|g| g.last_played.clone()).unwrap_or_default();
        let disabled_env     = existing.as_ref().map(|g| g.disabled_env_vars.clone()).unwrap_or_default();
        let disabled_flags   = existing.as_ref().map(|g| g.disabled_global_flags.clone()).unwrap_or_default();

        let install_dir = if install_dir.is_empty() {
            find_game_root(&exe_path)
                .or_else(|| Path::new(&exe_path).parent()
                    .map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_default()
        } else { install_dir };

        let g = GameConfig {
            name: name.clone(), exe_path, proton_version, proton_path,
            launch_args, custom_overrides, install_dir, env_vars,
            disabled_env_vars: disabled_env, prefix_path, fingerprint_lock,
            wrapper_command, exe_match_mode, enable_gamemode, enable_mangohud,
            enable_gamescope, disabled_global_flags: disabled_flags,
            gamescope_settings, playtime_seconds, last_played,
            game_file: file_path.to_string_lossy().into_owned(),
        };

        Self::write_file(&file_path, &g)
            .with_context(|| format!("saving {}", file_path.display()))?;
        self.scan_games();
        Ok(self.get(&name).cloned().unwrap_or(g))
    }

    pub fn remove_game(&mut self, name: &str) -> Result<()> {
        if let Some(g) = self.get(name) {
            let p = PathBuf::from(&g.game_file);
            if p.exists() { std::fs::remove_file(&p)?; }
        }
        self.scan_games();
        Ok(())
    }

    pub fn update_fields(&mut self, name: &str, f: impl FnOnce(&mut GameConfig)) -> Result<()> {
        let g = self.games.iter_mut().find(|g| g.name == name)
            .ok_or_else(|| anyhow::anyhow!("game not found: {name}"))?;
        f(g);
        let path = PathBuf::from(&g.game_file);
        let clone = g.clone();
        Self::write_file(&path, &clone)?;
        self.scan_games();
        Ok(())
    }

    pub fn rename_game(&mut self, old_name: &str, new_name: &str) -> Result<()> {
        let old_game = self.get(old_name).cloned()
            .ok_or_else(|| anyhow::anyhow!("game not found: {old_name}"))?;
        let old_path = PathBuf::from(&old_game.game_file);
        let new_path = Paths::games_dir().join(format!("{}.json", safe_name(new_name)));
        let mut updated = old_game;
        updated.name = new_name.to_string();
        updated.game_file = new_path.to_string_lossy().into_owned();
        Self::write_file(&new_path, &updated)?;
        if old_path != new_path && old_path.exists() {
            std::fs::remove_file(&old_path)?;
        }
        self.scan_games();
        Ok(())
    }

    pub fn record_playtime(&mut self, name: &str, secs: u64) {
        if secs == 0 { return; }
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let _ = self.update_fields(name, |g| {
            g.playtime_seconds += secs;
            g.last_played = now;
        });
    }

    pub fn clear_game_logs(&self, name: &str) -> bool {
        let dir = Paths::game_logs_dir(name);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "log").unwrap_or(false) {
                    let _ = std::fs::remove_file(p);
                }
            }
            true
        } else { false }
    }

    pub fn reset_prefix(&self, name: &str) -> bool {
        let g = match self.get(name) { Some(g) => g, None => return false };
        let prefix = if g.prefix_path.is_empty() {
            Paths::prefixes_dir().join(format!("{}prefix", safe_name(name)))
        } else {
            PathBuf::from(&g.prefix_path)
        };
        std::fs::remove_dir_all(&prefix).is_ok()
    }

    // ── Proton search ──────────────────────────────────────────────────────

    pub fn find_proton_in_dirs(&self, name: &str, extra: &[PathBuf]) -> Option<String> {
        if name.is_empty() { return None; }
        let default = dirs::home_dir()?.join(".steam/steam/compatibilitytools.d");
        for dir in std::iter::once(&default).chain(extra.iter()) {
            if let Some(p) = search_proton(dir, name) { return Some(p); }
        }
        None
    }

    pub fn scan_proton(&self, extra_dirs: &[PathBuf]) -> Vec<InstalledProton> {
        let default = dirs::home_dir()
            .map(|h| h.join(".steam/steam/compatibilitytools.d"))
            .unwrap_or_default();
        let mut result = Vec::new();
        for dir in std::iter::once(&default).chain(extra_dirs.iter()) {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            for entry in rd.flatten() {
                let path = entry.path();
                if !path.is_dir() || !path.join("proton").exists() { continue; }
                let nm = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let version = path.join("version")
                    .and_then_read_to_string()
                    .or_else(|| path.join("VERSION").and_then_read_to_string())
                    .unwrap_or_else(|| "unknown".into());
                let source = guess_source(&nm);
                result.push(InstalledProton {
                    name: nm, path: path.to_string_lossy().into_owned(),
                    version, source,
                });
            }
        }
        result.sort_by(|a, b| natural_sort_key(&b.name).cmp(&natural_sort_key(&a.name)));
        result
    }

    pub fn find_umu(&self) -> Option<String> {
        if let Ok(appdir) = std::env::var("APPDIR") {
            let p = PathBuf::from(appdir).join("usr/share/crucible/bootstrap/umu-run");
            if p.is_file() { return Some(p.to_string_lossy().into_owned()); }
        }
        let runner = Paths::runner_dir().join("umu-run");
        if runner.is_file() { return Some(runner.to_string_lossy().into_owned()); }
        which::which("umu-run").ok().map(|p| p.to_string_lossy().into_owned())
    }
}

trait AndThenReadToString {
    fn and_then_read_to_string(&self) -> Option<String>;
}
impl AndThenReadToString for PathBuf {
    fn and_then_read_to_string(&self) -> Option<String> {
        std::fs::read_to_string(self).ok().map(|s| s.trim().to_string())
    }
}

fn search_proton(dir: &Path, name: &str) -> Option<String> {
    if !dir.is_dir() { return None; }
    let mut candidates = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if !p.is_dir() || !p.join("proton").exists() { continue; }
        let dn = p.file_name()?.to_str()?.to_string();
        if dn == name { return Some(p.to_string_lossy().into_owned()); }
        if dn.contains(name) { candidates.push(p); }
    }
    if candidates.len() == 1 { Some(candidates[0].to_string_lossy().into_owned()) } else { None }
}

fn guess_source(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.starts_with("ge-proton") { "ge" }
    else if lower.starts_with("umu-proton") { "umu" }
    else if lower.contains("cachyos") { "cachy" }
    else { "ge" }
    .into()
}

fn natural_sort_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        if chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            let num: String = chars.by_ref().take_while(|c| c.is_ascii_digit()).collect();
            key.push((1, num.parse::<u64>().unwrap_or(0), String::new()));
        } else {
            let txt: String = chars.by_ref().take_while(|c| !c.is_ascii_digit()).collect();
            key.push((0, 0, txt.to_lowercase()));
        }
    }
    key
}
