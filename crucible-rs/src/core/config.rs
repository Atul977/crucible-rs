// src/core/config.rs — Global defaults + app preferences.

use std::collections::HashMap;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::core::paths::Paths;

// ── Gamescope ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GamescopeSettings {
    #[serde(default)] pub enable_force_grab_cursor: bool,
    #[serde(default)] pub window_type: String,
    #[serde(default)] pub game_width: String,
    #[serde(default)] pub game_height: String,
    #[serde(default)] pub upscale_width: String,
    #[serde(default)] pub upscale_height: String,
    #[serde(default)] pub upscale_method: String,
    #[serde(default)] pub fps_limiter: String,
    #[serde(default)] pub fps_limiter_no_focus: String,
    #[serde(default)] pub additional_options: String,
}

// ── App preferences ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPrefs {
    #[serde(default = "t")]    pub minimize_to_tray: bool,
    #[serde(default = "t")]    pub restore_geometry: bool,
    #[serde(default = "t")]    pub auto_update_umu: bool,
    #[serde(default)]          pub custom_proton_dir: String,
    #[serde(default = "info")] pub log_level: String,
    #[serde(default = "w1200")]pub window_width: i32,
    #[serde(default = "h750")] pub window_height: i32,
    #[serde(default)]          pub window_x: i32,
    #[serde(default)]          pub window_y: i32,
}
fn t() -> bool { true }
fn info() -> String { "info".into() }
fn w1200() -> i32 { 1200 }
fn h750() -> i32 { 750 }

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            minimize_to_tray: true, restore_geometry: true, auto_update_umu: true,
            custom_proton_dir: String::new(), log_level: "info".into(),
            window_width: 1200, window_height: 750, window_x: 0, window_y: 0,
        }
    }
}

impl AppPrefs {
    pub fn load() -> Self {
        std::fs::read_to_string(Paths::app_prefs_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Paths::app_prefs_path(), t);
        }
    }
}

// ── Global game config ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)] pub proton_version: String,
    #[serde(default)] pub launch_args: String,
    #[serde(default)] pub custom_overrides: String,
    #[serde(default)] pub env_vars: HashMap<String, String>,
    #[serde(default)] pub wrapper_command: String,
    #[serde(default)] pub fingerprint_lock: bool,
    #[serde(default)] pub enable_gamemode: bool,
    #[serde(default)] pub enable_mangohud: bool,
    #[serde(default)] pub enable_gamescope: bool,
    #[serde(default)] pub gamescope_settings: GamescopeSettings,
}

impl GlobalConfig {
    pub fn load() -> Self {
        std::fs::read_to_string(Paths::config_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<()> {
        let t = serde_json::to_string_pretty(self)?;
        std::fs::write(Paths::config_path(), t)?;
        Ok(())
    }

    /// Merge global defaults under game-specific values.
    pub fn resolve(&self, game: &crate::core::game::GameConfig) -> crate::core::game::GameConfig {
        let mut r = game.clone();
        let disabled: std::collections::HashSet<&str> =
            game.disabled_global_flags.iter().map(String::as_str).collect();

        macro_rules! inherit_str {
            ($field:ident) => {
                if r.$field.is_empty() && !self.$field.is_empty() {
                    r.$field = self.$field.clone();
                }
            };
        }
        inherit_str!(proton_version);
        inherit_str!(launch_args);
        inherit_str!(custom_overrides);
        inherit_str!(wrapper_command);

        for flag in ["fingerprint_lock","enable_gamemode","enable_mangohud","enable_gamescope"] {
            if !disabled.contains(flag) {
                match flag {
                    "fingerprint_lock"  => { if !game.fingerprint_lock  { r.fingerprint_lock  = self.fingerprint_lock; } }
                    "enable_gamemode"   => { if !game.enable_gamemode   { r.enable_gamemode   = self.enable_gamemode; } }
                    "enable_mangohud"   => { if !game.enable_mangohud   { r.enable_mangohud   = self.enable_mangohud; } }
                    "enable_gamescope"  => { if !game.enable_gamescope  { r.enable_gamescope  = self.enable_gamescope; } }
                    _ => {}
                }
            }
        }

        // env_vars: global base, game overrides on top, disabled_env_vars removed from global
        let mut env = self.env_vars.clone();
        for k in &game.disabled_env_vars { env.remove(k); }
        env.extend(game.env_vars.clone());
        r.env_vars = env;

        r
    }
}
