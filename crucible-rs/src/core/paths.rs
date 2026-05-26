// src/core/paths.rs — XDG paths, safe_name, env cleanup, prefix validation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use regex::Regex;

// ── Directory layout ───────────────────────────────────────────────────────

pub struct Paths;

impl Paths {
    pub fn data_dir() -> PathBuf {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("crucible-launcher");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
    pub fn games_dir() -> PathBuf     { mk(Self::data_dir().join("games")) }
    pub fn prefixes_dir() -> PathBuf  { mk(Self::data_dir().join("Prefix")) }
    pub fn logs_dir() -> PathBuf      { mk(Self::data_dir().join("logs")) }
    pub fn app_logs_dir() -> PathBuf  { mk(Self::logs_dir().join("app")) }
    pub fn artwork_dir() -> PathBuf   { mk(Self::data_dir().join("artwork")) }
    pub fn runner_dir() -> PathBuf    { mk(Self::data_dir().join("runner")) }
    pub fn fingerprints_dir() -> PathBuf { mk(Self::data_dir().join("fingerprints")) }
    pub fn config_path() -> PathBuf   { Self::data_dir().join("global_config.json") }
    pub fn app_prefs_path() -> PathBuf { Self::data_dir().join("app_prefs.json") }

    pub fn game_logs_dir(game_name: &str) -> PathBuf {
        mk(Self::logs_dir().join("games").join(safe_name(game_name)))
    }
}

fn mk(p: PathBuf) -> PathBuf { let _ = std::fs::create_dir_all(&p); p }

// ── Name helpers ──────────────────────────────────────────────────────────

pub fn safe_name(name: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[^a-zA-Z0-9]+").unwrap());
    re.replace_all(&name.to_lowercase(), "_")
        .trim_matches('_')
        .to_string()
}

pub fn display_name_from_exe(exe_path: &str) -> String {
    let stem = Path::new(exe_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");
    stem.split(|c: char| c == '_' || c == '-' || c == ' ')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Game-root detection ───────────────────────────────────────────────────

static GAME_SUBDIRS: std::sync::OnceLock<HashSet<&'static str>> = std::sync::OnceLock::new();

fn game_subdirs() -> &'static HashSet<&'static str> {
    GAME_SUBDIRS.get_or_init(|| {
        ["binaries","bin","win64","win32","x64","x86","x86_64","windows",
         "engine","game","content","app","apps","build","shipping",
         "development","programs"].into_iter().collect()
    })
}

pub static GENERIC_DIRS: std::sync::OnceLock<HashSet<&'static str>> = std::sync::OnceLock::new();
pub fn generic_dirs() -> &'static HashSet<&'static str> {
    GENERIC_DIRS.get_or_init(|| {
        let mut s = game_subdirs().clone();
        s.extend(["program files","program files (x86)","steamapps",
                   "common","games","steam","library"]);
        s
    })
}

pub fn find_game_root(exe_path: &str) -> Option<String> {
    let path = Path::new(exe_path);
    if !path.exists() { return None; }
    let subdirs = game_subdirs();
    let mut cur = path.parent()?;
    for _ in 0..16 {
        if cur.parent() == Some(cur) { break; }
        let name = cur.file_name()?.to_str()?.to_lowercase();
        if subdirs.contains(name.as_str()) {
            cur = cur.parent()?;
        } else {
            return Some(cur.to_string_lossy().into_owned());
        }
    }
    None
}

pub fn find_app_id_in_game_dir(game_root: &str) -> Option<String> {
    let root = Path::new(game_root);
    if !root.is_dir() { return None; }
    for entry in walkdir::WalkDir::new(root).max_depth(10).follow_links(false) {
        let Ok(e) = entry else { continue };
        if e.file_name().to_str() != Some("steam_appid.txt") { continue; }
        if let Ok(txt) = std::fs::read_to_string(e.path()) {
            let t = txt.trim().to_string();
            if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
                return Some(t);
            }
        }
    }
    None
}

// ── Environment cleanup ───────────────────────────────────────────────────

const SAVED_KEYS: &[&str] = &["LD_LIBRARY_PATH","PYTHONHOME","PYTHONPATH","QT_PLUGIN_PATH","PATH"];
const EXTRA_AI_KEYS: &[&str] = &["QT_QPA_PLATFORM_PLUGIN_PATH","QT_QPA_FONTDIR","QTDIR",
                                   "QT_XKB_CONFIG_ROOT","LD_PRELOAD"];
const IDENTITY_KEYS: &[&str] = &["APPIMAGE","APPDIR","OWD","ARGV0"];
const DESKTOP_KEYS: &[&str]  = &["DESKTOP_STARTUP_ID","XDG_ACTIVATION_TOKEN",
                                   "BAMF_DESKTOP_FILE_HINT","GIO_LAUNCHED_DESKTOP_FILE",
                                   "GIO_LAUNCHED_DESKTOP_FILE_PID"];

pub fn is_appimage() -> bool { std::env::var("APPDIR").is_ok() }

pub fn clean_env() -> std::collections::HashMap<String, String> {
    let mut env: std::collections::HashMap<String, String> = std::env::vars().collect();
    for k in DESKTOP_KEYS  { env.remove(*k); }
    if is_appimage() {
        for k in SAVED_KEYS  { restore_or_remove(&mut env, k); }
        for k in EXTRA_AI_KEYS { env.remove(*k); }
        for k in IDENTITY_KEYS { env.remove(*k); }
    }
    env.retain(|k, _| !k.starts_with("CRUCIBLE_ORIG_"));
    env
}

fn restore_or_remove(env: &mut std::collections::HashMap<String, String>, key: &str) {
    let orig = format!("CRUCIBLE_ORIG_{key}");
    match env.remove(&orig) {
        Some(v) if !v.is_empty() => { env.insert(key.to_string(), v); }
        _ => { env.remove(key); }
    }
}

// ── Prefix validation ─────────────────────────────────────────────────────

pub fn validate_prefix(prefix: &Path) -> Option<String> {
    let Ok(mut rd) = std::fs::read_dir(prefix) else { return None };
    if rd.next().is_none() { return None; } // fresh — OK
    let required = ["dosdevices","drive_c","system.reg","user.reg","userdef.reg"];
    let missing: Vec<_> = required.iter().filter(|&&e| !prefix.join(e).exists()).collect();
    if missing.is_empty() { None }
    else {
        Some(format!(
            "Wine prefix appears corrupted — missing: {}\nPath: {}",
             missing.iter().map(|&&s| s).collect::<Vec<&str>>().join(", "),
             prefix.display()
        ))
    }
}
