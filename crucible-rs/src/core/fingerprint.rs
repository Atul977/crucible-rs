// src/core/fingerprint.rs — bwrap /proc fingerprint isolation.

use std::path::{Path, PathBuf};
use crate::core::paths::{Paths, safe_name};

pub struct FingerprintManager {
    dir: PathBuf,
}

impl FingerprintManager {
    pub fn new() -> Self {
        Self { dir: Paths::fingerprints_dir() }
    }

    fn snap_dir(&self, name: &str) -> PathBuf { self.dir.join(safe_name(name)) }

    pub fn snapshot(&self, game_name: &str) -> bool {
        let dir = self.snap_dir(game_name);
        let _ = std::fs::create_dir_all(&dir);
        let ok_cpu = std::fs::copy("/proc/cpuinfo", dir.join("cpuinfo")).is_ok();
        let ok_ver = std::fs::copy("/proc/version", dir.join("version")).is_ok();
        ok_cpu && ok_ver
    }

    pub fn clear(&self, game_name: &str) {
        let _ = std::fs::remove_dir_all(self.snap_dir(game_name));
    }

    pub fn bwrap_args(&self, game_name: &str) -> Vec<String> {
        let bwrap = match which::which("bwrap") {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(_) => {
                log::warn!("fingerprint_lock enabled but bwrap not found");
                return vec![];
            }
        };
        let snap = self.snap_dir(game_name);
        let cpu  = snap.join("cpuinfo");
        let ver  = snap.join("version");
        if !cpu.exists() { self.snapshot(game_name); }

        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
        let mut args = vec![
            bwrap,
            "--ro-bind".into(), "/".into(), "/".into(),
            "--dev-bind".into(), "/dev".into(), "/dev".into(),
            "--proc".into(), "/proc".into(),
            "--bind".into(), "/tmp".into(), "/tmp".into(),
            "--bind".into(), "/run".into(), "/run".into(),
            "--bind".into(), home.to_string_lossy().into_owned(),
                             home.to_string_lossy().into_owned(),
        ];
        if cpu.exists() {
            args.extend(["--ro-bind".into(), cpu.to_string_lossy().into_owned(),
                          "/proc/cpuinfo".into()]);
        }
        if ver.exists() {
            args.extend(["--ro-bind".into(), ver.to_string_lossy().into_owned(),
                          "/proc/version".into()]);
        }
        args.push("--".into());
        args
    }
}
