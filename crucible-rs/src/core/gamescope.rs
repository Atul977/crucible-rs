// src/core/gamescope.rs — Build gamescope command-line prefix.

use crate::core::config::GamescopeSettings;

fn has_filter_flag(bin: &str) -> bool {
    std::process::Command::new(bin).arg("--help")
        .output().ok()
        .map(|o| {
            let txt = String::from_utf8_lossy(&o.stdout).to_string()
                + &String::from_utf8_lossy(&o.stderr);
            txt.contains("-F, --filter")
        })
        .unwrap_or(false)
}

pub fn build_gamescope_cmd(s: &GamescopeSettings) -> Vec<String> {
    let has_any = !s.game_width.is_empty() || !s.game_height.is_empty()
        || !s.upscale_width.is_empty() || !s.upscale_height.is_empty()
        || !s.upscale_method.is_empty() || !s.window_type.is_empty()
        || !s.fps_limiter.is_empty() || !s.fps_limiter_no_focus.is_empty()
        || s.enable_force_grab_cursor || !s.additional_options.is_empty();
    if !has_any { return vec![]; }

    let bin = match which::which("gamescope") {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => { log::warn!("gamescope enabled but not found on PATH"); return vec![]; }
    };
    let new = has_filter_flag(&bin);
    let mut cmd = vec![bin];

    macro_rules! flag {
        ($cond:expr, $($arg:expr),+) => { if $cond { $(cmd.push($arg.to_string());)+ } };
    }

    flag!(!s.game_width.is_empty(),  "-w", &s.game_width);
    flag!(!s.game_height.is_empty(), "-h", &s.game_height);
    flag!(!s.upscale_width.is_empty(),  "-W", &s.upscale_width);
    flag!(!s.upscale_height.is_empty(), "-H", &s.upscale_height);

    match s.upscale_method.to_lowercase().as_str() {
        "fsr"     => if new { cmd.extend(["-F".into(),"fsr".into()]) } else { cmd.push("-U".into()) },
        "nis"     => if new { cmd.extend(["-F".into(),"nis".into()]) } else { cmd.push("-Y".into()) },
        "integer" => if new { cmd.extend(["-S".into(),"integer".into()]) } else { cmd.push("-i".into()) },
        "stretch" => if new { cmd.extend(["-S".into(),"stretch".into()]) },
        _ => {}
    }
    match s.window_type.to_lowercase().as_str() {
        "fullscreen" => cmd.push("-f".into()),
        "borderless" => cmd.push("-b".into()),
        _ => {}
    }
    flag!(!s.fps_limiter.is_empty(),          "-r", &s.fps_limiter);
    flag!(!s.fps_limiter_no_focus.is_empty(), "-o", &s.fps_limiter_no_focus);
    if s.enable_force_grab_cursor { cmd.push("--force-grab-cursor".into()); }

    if !s.additional_options.is_empty() {
        match shlex::split(&s.additional_options) {
            Some(extra) => cmd.extend(extra),
            None => log::warn!("failed to parse gamescope additional_options"),
        }
    }

    cmd.push("--".into());
    cmd
}
