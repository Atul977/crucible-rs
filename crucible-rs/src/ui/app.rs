// src/ui/app.rs — Application bootstrap and single-instance guard.

use gtk4::prelude::*;
use libadwaita::prelude::*;
use crate::ui::state::AppState;
use crate::ui::window::MainWindow;

pub fn run() -> glib::ExitCode {
    // Single-instance via GApplication
    let app = libadwaita::Application::builder()
        .application_id("io.github.northmind.crucible")
        .flags(gio::ApplicationFlags::empty())
        .build();

    app.connect_startup(|app| {
        // Load the Adwaita stylesheet (dark by default — matches original)
        let mgr = libadwaita::StyleManager::default();
        mgr.set_color_scheme(libadwaita::ColorScheme::ForceDark);
    });

    app.connect_activate(|app| {
        // Only create the window once
        if let Some(win) = app.active_window() {
            win.present();
            return;
        }
        let state = AppState::new();
        let win = MainWindow::new(app, state);
        win.present();
    });

    app.run()
}
