// src/main.rs — Crucible entry point.
//
// Handles:
//   --launch "Game Name"  →  launch a game headlessly (used by .desktop shortcuts)
//   (no args)             →  show the GUI

mod core;
mod ui;

use std::process;

fn main() {
    // Logging
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "crucible=info,warn");
    }
    env_logger::init();

    // Seed the bundled umu-run if present
    seed_umu_run();

    // Kick off silent umu-run update in background (non-blocking)
    let auto_update = crate::core::config::AppPrefs::load().auto_update_umu;
    if auto_update {
        std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().expect("tokio rt");
            if let Err(e) = rt.block_on(crate::core::umu_update::update_umu()) {
                log::warn!("umu-run update: {e}");
            }
        });
    }

    // Check for CLI flags
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--launch") {
        if let Some(name) = args.get(pos + 1) {
            headless_launch(name);
            return;
        }
        eprintln!("--launch requires a game name");
        process::exit(1);
    }

    // GUI
    let code = ui::app::run();
    process::exit(code.value());
}

/// Launch a game without the GUI (used from .desktop shortcuts).
fn headless_launch(name: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all().build().expect("tokio rt");
    let state = ui::state::AppState::new();
    let mut st = state.lock().unwrap();
    if let Err(e) = st.launcher.launch(&st.gm, name) {
        eprintln!("Failed to launch '{name}': {e}");
        process::exit(1);
    }
    drop(st);
    // Block until the game exits by polling
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut st2 = state.lock().unwrap();
        let exited = st2.launcher.poll_exits();
        for exited_name in exited {
            let secs = st2.launcher.on_exited(&exited_name);
            st2.gm.record_playtime(&exited_name, secs);
            process::exit(0);
        }
    }
}

fn seed_umu_run() {
    let dest = crate::core::paths::Paths::runner_dir().join("umu-run");
    if dest.is_file() && dest.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return;
    }
    let Ok(appdir) = std::env::var("APPDIR") else { return };
    let src = std::path::PathBuf::from(appdir)
        .join("usr/share/crucible/bootstrap/umu-run");
    if !src.is_file() { return; }
    if std::fs::copy(&src, &dest).is_ok() {
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest,
                std::fs::Permissions::from_mode(0o755));
        }
        log::info!("Seeded bundled umu-run → {}", dest.display());
    }
}
