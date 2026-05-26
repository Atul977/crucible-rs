// src/core/launcher.rs — Game launch pipeline: validate → prepare → execute.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use anyhow::{bail, Result};
use uuid::Uuid;

use crate::core::fingerprint::FingerprintManager;
use crate::core::game::{build_dll_overrides, GameConfig, GameManager};
use crate::core::gamescope::build_gamescope_cmd;
use crate::core::paths::{clean_env, safe_name, validate_prefix, Paths};
use crate::core::process::{
    detached_fork, descendants, inhibit_screensaver, pid_alive,
    scan_uuid_pids, stop_pids, uninhibit_screensaver,
};
use crate::core::shortcut::ShortcutManager;

// ── State machine ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum GameState { Idle, Launching, Running, Stopping }

struct RunEntry {
    pid: u32,
    uuid: String,
    ss_cookie: Option<u32>,
    started_at: Instant,
    stopping: bool,
}

// ── Launcher ──────────────────────────────────────────────────────────────

pub struct Launcher {
    state: HashMap<String, GameState>,
    running: HashMap<String, RunEntry>,
    fingerprint: FingerprintManager,
    extra_proton_dirs: Vec<PathBuf>,
    pub on_game_started: Option<Box<dyn Fn(&str) + Send + 'static>>,
    pub on_game_stopped: Option<Box<dyn Fn(&str) + Send + 'static>>,
}

impl Launcher {
    pub fn new(extra_proton_dirs: Vec<PathBuf>) -> Self {
        Self {
            state: HashMap::new(),
            running: HashMap::new(),
            fingerprint: FingerprintManager::new(),
            extra_proton_dirs,
            on_game_started: None,
            on_game_stopped: None,
        }
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.state.get(name).map(|s| *s == GameState::Running || *s == GameState::Launching)
            .unwrap_or(false)
    }

    pub fn get_state(&self, name: &str) -> GameState {
        self.state.get(name).cloned().unwrap_or(GameState::Idle)
    }

    // ── Launch ─────────────────────────────────────────────────────────────

    pub fn launch(
        &mut self,
        gm: &GameManager,
        name: &str,
    ) -> Result<()> {
        // stale-state check
        if let Some(GameState::Running | GameState::Stopping) = self.state.get(name) {
            if let Some(entry) = self.running.get(name) {
                if !pid_alive(entry.pid) && scan_uuid_pids(&entry.uuid).is_empty() {
                    self.on_exited(name);
                }
            }
        }

        match self.state.get(name).unwrap_or(&GameState::Idle) {
            GameState::Idle => {}
            s => bail!("Game '{name}' is already {s:?}"),
        }

        let game = gm.get(name).ok_or_else(|| anyhow::anyhow!("game not found: {name}"))?;
        let resolved = gm.global_config.resolve(game);

        // ── Validate ──────────────────────────────────────────────────────
        let exe = &resolved.exe_path;
        if !Path::new(exe).exists() { bail!("Executable not found:\n{exe}"); }

        let proton_path = gm.find_proton_in_dirs(&resolved.proton_version, &self.extra_proton_dirs)
            .or_else(|| {
                if !resolved.proton_path.is_empty() { Some(resolved.proton_path.clone()) }
                else { None }
            })
            .ok_or_else(|| anyhow::anyhow!(
                "No Proton version configured.\nEdit the game and select a Proton version."
            ))?;

        if !Path::new(&proton_path).is_dir() {
            bail!("Proton path not found: {proton_path}");
        }

        let umu = gm.find_umu().ok_or_else(|| anyhow::anyhow!(
            "umu-run not found.\nInstall it or check that it is bundled with the AppImage."
        ))?;

        // nofile limit check (for esync/fsync)
        if resolved.env_vars.get("PROTON_NO_ESYNC").map(|v| v != "1").unwrap_or(true)
            || resolved.env_vars.get("PROTON_NO_FSYNC").map(|v| v != "1").unwrap_or(true)
        {
            let (_soft, hard) = nix::sys::resource::getrlimit(nix::sys::resource::Resource::RLIMIT_NOFILE)
                .unwrap_or((0, 0));
            if hard != u64::MAX && hard < 524288 {
                bail!(
                    "ESYNC/FSYNC require a file-descriptor limit of at least 524288.\n\
                     Current hard limit: {hard}\n\n\
                     Add to /etc/systemd/user.conf:\n  DefaultLimitNOFILE=1048576\n\n\
                     Or disable ESYNC/FSYNC in the game's Advanced tab."
                );
            }
        }

        self.state.insert(name.to_string(), GameState::Launching);

        // ── Prefix ────────────────────────────────────────────────────────
        let sname = safe_name(name);
        let prefix_path = if resolved.prefix_path.is_empty() {
            Paths::prefixes_dir().join(format!("{sname}prefix"))
        } else {
            PathBuf::from(&resolved.prefix_path)
        };
        let _ = std::fs::create_dir_all(&prefix_path);
        if let Some(err) = validate_prefix(&prefix_path) {
            self.state.remove(name);
            bail!("{err}");
        }

        // ── Log dir ───────────────────────────────────────────────────────
        let log_dir = Paths::game_logs_dir(name);
        for old in std::fs::read_dir(&log_dir).into_iter().flatten().flatten() {
            if old.path().extension().map(|e| e == "log").unwrap_or(false) {
                let _ = std::fs::remove_file(old.path());
            }
        }
        let log_path = log_dir.join(format!(
            "{}.log", chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));

        // ── Env ───────────────────────────────────────────────────────────
        let game_uuid = Uuid::new_v4().to_string();
        let raw_appid = crate::core::paths::find_app_id_in_game_dir(&resolved.install_dir)
            .or_else(|| crate::core::paths::find_app_id_in_game_dir(
                &Path::new(exe).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()))
            .unwrap_or_else(|| "umu-default".into());

        let steam_id = if raw_appid == "umu-default" {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            name.hash(&mut h);
            format!("{}", h.finish() % 900_000 + 100_000)
        } else { raw_appid };

        let mut env = clean_env();
        env.extend(resolved.env_vars.iter().map(|(k,v)|(k.clone(),v.clone())));
        env.retain(|k, _| !k.starts_with("BASH_FUNC"));

        env.insert("GAMEID".into(), steam_id.clone());
        env.insert("SteamAppId".into(), steam_id.clone());
        env.insert("SteamGameId".into(), steam_id.clone());
        env.insert("PROTONPATH".into(), proton_path);
        env.insert("WINEPREFIX".into(), prefix_path.to_string_lossy().into_owned());
        env.insert("WINEARCH".into(), "win64".into());
        env.insert("PROTON_VERB".into(), "waitforexitandrun".into());
        env.insert("WINEDLLOVERRIDES".into(), build_dll_overrides(&resolved.custom_overrides));
        env.entry("WINEDEBUG".into()).or_insert_with(|| "-all".into());
        env.entry("DXVK_LOG_LEVEL".into()).or_insert_with(|| "error".into());
        env.entry("PULSE_LATENCY_MSEC".into()).or_insert_with(|| "60".into());

        if env.get("PROTON_ENABLE_NVAPI").map(|v| v == "1").unwrap_or(false) {
            env.entry("DXVK_ENABLE_NVAPI".into()).or_insert_with(|| "1".into());
            env.entry("DXVK_NVAPI_ALLOW_OTHER_DRIVERS".into()).or_insert_with(|| "1".into());
        }
        if let Ok(lc) = std::env::var("LC_ALL") {
            env.entry("HOST_LC_ALL".into()).or_insert(lc);
        }
        env.insert("CRUCIBLE_GAME_ID".into(), game_uuid.clone());

        // ── Command ───────────────────────────────────────────────────────
        let exe_args: Vec<String> = if resolved.launch_args.is_empty() { vec![] }
            else { shlex::split(&resolved.launch_args).unwrap_or_default() };

        let mut cmd = vec![umu.clone(), exe.clone()];
        cmd.extend(exe_args);

        // fingerprint bwrap
        if resolved.fingerprint_lock {
            let bwrap = self.fingerprint.bwrap_args(name);
            if !bwrap.is_empty() { cmd = [bwrap, cmd].concat(); }
        }
        // wrapper
        if !resolved.wrapper_command.is_empty() {
            if let Some(wrapper) = shlex::split(&resolved.wrapper_command) {
                cmd = [wrapper, cmd].concat();
            }
        }
        // gamemode
        if resolved.enable_gamemode && which::which("gamemoderun").is_ok() {
            cmd.insert(0, "gamemoderun".into());
        }
        // mangohud
        if resolved.enable_mangohud && which::which("mangohud").is_ok() {
            cmd.insert(0, "mangohud".into());
        }
        // systemd-run scope (clean cgroup)
        if which::which("systemd-run").is_ok() {
            let unit = format!("game-{}-{}", sname, &game_uuid[..8]);
            cmd = [vec!["systemd-run".into(),"--user".into(),"--scope".into(),
                        format!("--unit={unit}"), "--".into()], cmd].concat();
        }
        // gamescope (outermost)
        if resolved.enable_gamescope {
            let gs = build_gamescope_cmd(&resolved.gamescope_settings);
            if !gs.is_empty() { cmd = [gs, cmd].concat(); }
        }

        // ── cwd ───────────────────────────────────────────────────────────
        let cwd = if !resolved.install_dir.is_empty() && Path::new(&resolved.install_dir).is_dir() {
            resolved.install_dir.clone()
        } else {
            Path::new(exe).parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        };

        // ── Fork ──────────────────────────────────────────────────────────
        let pid = detached_fork(&cmd, &env, &cwd, &log_path);
        if pid == 0 {
            self.state.remove(name);
            bail!("Failed to launch game process");
        }

        let ss = inhibit_screensaver();
        self.running.insert(name.to_string(), RunEntry {
            pid, uuid: game_uuid.clone(), ss_cookie: ss,
            started_at: Instant::now(), stopping: false,
        });
        self.state.insert(name.to_string(), GameState::Running);

        if let Some(cb) = &self.on_game_started {
            (cb)(name);
        }

        // background watcher thread
        let name_owned = name.to_string();
        let uuid_owned = game_uuid;
        let cb_stop = self.on_game_stopped.as_ref().map(|_| name_owned.clone());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if pid_alive(pid) { continue; }
                if !scan_uuid_pids(&uuid_owned).is_empty() { continue; }
                break;
            }
            // signal the main thread via a flag file so we don't need cross-thread callbacks
            let flag = Paths::data_dir().join(format!(".exited_{name_owned}"));
            let _ = std::fs::write(&flag, &[]);
        });

        Ok(())
    }

    // ── Stop ───────────────────────────────────────────────────────────────

    pub fn stop(&mut self, name: &str) -> bool {
        let entry = match self.running.get_mut(name) {
            Some(e) => { e.stopping = true; e }
            None => return false,
        };

        self.state.insert(name.to_string(), GameState::Stopping);

        let pid = entry.pid;
        let uuid = entry.uuid.clone();
        let cookie = entry.ss_cookie;

        if let Some(c) = cookie { uninhibit_screensaver(c); }

        let mut pids: HashSet<u32> = scan_uuid_pids(&uuid);
        pids.insert(pid);
        pids.extend(descendants(pid));
        let pids: Vec<u32> = pids.into_iter().collect();

        let name_owned = name.to_string();
        stop_pids(&pids, move || {
            let flag = Paths::data_dir().join(format!(".exited_{name_owned}"));
            let _ = std::fs::write(&flag, &[]);
        });
        true
    }

    // ── Called when we detect exit (flag file polling) ────────────────────

    pub fn on_exited(&mut self, name: &str) -> u64 {
        let elapsed = self.running.remove(name)
            .map(|e| e.started_at.elapsed().as_secs())
            .unwrap_or(0);
        if let Some(c) = self.running.get(name).and_then(|e| e.ss_cookie) {
            uninhibit_screensaver(c);
        }
        self.state.remove(name);
        // Remove the flag file
        let flag = Paths::data_dir().join(format!(".exited_{name}"));
        let _ = std::fs::remove_file(&flag);
        if let Some(cb) = &self.on_game_stopped {
            (cb)(name);
        }
        elapsed
    }

    /// Call periodically (e.g. every second) to poll for exited games.
    /// Returns list of names that exited.
    pub fn poll_exits(&mut self) -> Vec<String> {
        let data_dir = Paths::data_dir();
        let mut exited = Vec::new();
        for name in self.running.keys().cloned().collect::<Vec<_>>() {
            let flag = data_dir.join(format!(".exited_{name}"));
            if flag.exists() { exited.push(name); }
        }
        exited
    }

    pub fn set_extra_proton_dirs(&mut self, dirs: Vec<PathBuf>) {
        self.extra_proton_dirs = dirs;
    }
}

// ── Winetricks ────────────────────────────────────────────────────────────

pub fn launch_winetricks(
    gm: &GameManager,
    prefix_path: &str,
    proton_name: Option<&str>,
    extra_dirs: &[PathBuf],
) -> Result<()> {
    let umu = gm.find_umu().ok_or_else(|| anyhow::anyhow!("umu-run not found"))?;
    let ppath = proton_name
        .and_then(|n| gm.find_proton_in_dirs(n, extra_dirs))
        .ok_or_else(|| anyhow::anyhow!("Proton not found"))?;

    let _ = std::fs::create_dir_all(prefix_path);
    let mut env = clean_env();
    env.insert("WINEPREFIX".into(), prefix_path.to_string());
    env.insert("PROTONPATH".into(), ppath);
    env.insert("GAMEID".into(), "0".into());

    std::process::Command::new(&umu)
        .args(["winetricks", "--gui"])
        .envs(&env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}
