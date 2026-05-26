// src/core/shortcut.rs — Desktop shortcut creation for games.

use std::path::{Path, PathBuf};
use anyhow::Result;
use crate::core::game::GameConfig;
use crate::core::paths::{Paths, safe_name};

pub struct ShortcutManager;

impl ShortcutManager {
    fn desktop_dir() -> PathBuf {
        dirs::home_dir().unwrap_or_default().join(".local/share/applications")
    }

    pub fn game_desktop_path(name: &str) -> PathBuf {
        Self::desktop_dir().join(format!("crucible-{}.desktop", safe_name(name)))
    }

    pub fn has_shortcut(name: &str) -> bool { Self::game_desktop_path(name).exists() }

    pub fn create(game: &GameConfig) -> Result<PathBuf> {
        let name = &game.name;
        let dir = Self::desktop_dir();
        let _ = std::fs::create_dir_all(&dir);

        let exec = Self::exec_line(name);
        let icon = Self::find_icon(game);
        let sname = safe_name(name);

        // Sanitise name for .desktop
        let display_name: String = name.chars()
            .filter(|c| c.is_alphanumeric() || " .-_()".contains(*c))
            .collect();

        let content = format!(
            "[Desktop Entry]\nType=Application\nName={display_name}\n\
             Exec={exec}\nIcon={icon}\nCategories=Game;\n\
             StartupWMClass=crucible-{sname}\nStartupNotify=false\n"
        );

        let path = Self::game_desktop_path(name);
        std::fs::write(&path, &content)?;
        // Make executable
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        Self::refresh_db();
        Ok(path)
    }

    pub fn remove(name: &str) {
        let p = Self::game_desktop_path(name);
        let _ = std::fs::remove_file(p);
        Self::refresh_db();
    }

    fn exec_line(name: &str) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\\\""));
        let qname = q(name);
        // AppImage path
        if let Ok(ai) = std::env::var("APPIMAGE") {
            if Path::new(&ai).exists() {
                return format!("{} --launch {qname}", q(&ai));
            }
        }
        if let Ok(p) = which::which("crucible") {
            return format!("{} --launch {qname}", q(&p.to_string_lossy()));
        }
        format!("crucible --launch {qname}")
    }

    fn find_icon(game: &GameConfig) -> String {
        // Try cover art
        for candidate in [game.cover_path(), game.header_path()] {
            if candidate.exists() {
                return candidate.to_string_lossy().into_owned();
            }
        }
        "crucible".into()
    }

    fn refresh_db() {
        let dir = Self::desktop_dir();
        let _ = std::process::Command::new("update-desktop-database")
            .arg(dir).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .status();
    }

    // Ensure the launcher's own .desktop file exists (AppImage mode)
    pub fn ensure_launcher_desktop() {
        let Ok(appimage) = std::env::var("APPIMAGE") else { return };
        if !Path::new(&appimage).exists() { return }
        let dir = Self::desktop_dir();
        let _ = std::fs::create_dir_all(&dir);
        let icon: String = if let Ok(appdir) = std::env::var("APPDIR") {
            let src = PathBuf::from(appdir).join("crucible.png");
            if src.exists() {
                let dst = dirs::home_dir().unwrap_or_default()
                    .join(".local/share/icons/hicolor/256x256/apps/crucible.png");
                let _ = std::fs::create_dir_all(dst.parent().unwrap());
                let _ = std::fs::copy(&src, &dst);
                "crucible".into()
            } else { "crucible".into() }
        } else { "crucible".into() };
        let content = format!(
            "[Desktop Entry]\nName=Crucible\nExec=\"{appimage}\"\nIcon={icon}\n\
             Type=Application\nCategories=Game;\nComment=Wine/Proton game launcher\n\
             StartupWMClass=crucible\nStartupNotify=true\n"
        );
        let path = dir.join("crucible.desktop");
        let _ = std::fs::write(&path, &content);
        Self::refresh_db();
    }
}
