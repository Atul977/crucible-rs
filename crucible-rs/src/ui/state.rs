// src/ui/state.rs — Shared application state (Arc<Mutex<AppState>>).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use crate::core::config::{AppPrefs, GlobalConfig};
use crate::core::game::GameManager;
use crate::core::launcher::Launcher;

pub type SharedState = Arc<Mutex<AppState>>;

pub struct AppState {
    pub gm: GameManager,
    pub launcher: Launcher,
    pub prefs: AppPrefs,
    pub extra_proton_dirs: Vec<PathBuf>,
}

impl AppState {
    pub fn new() -> SharedState {
        let prefs = AppPrefs::load();
        let extra_proton_dirs: Vec<PathBuf> = if prefs.custom_proton_dir.is_empty() {
            vec![]
        } else {
            vec![PathBuf::from(&prefs.custom_proton_dir)]
        };
        let gm = GameManager::new();
        let launcher = Launcher::new(extra_proton_dirs.clone());

        crate::core::shortcut::ShortcutManager::ensure_launcher_desktop();

        Arc::new(Mutex::new(AppState { gm, launcher, prefs, extra_proton_dirs }))
    }
}
