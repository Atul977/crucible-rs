// src/ui/window.rs — AdwApplicationWindow with sidebar navigation.

use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::sync::{Arc, Mutex};
use crate::ui::library::LibraryPage;
use crate::ui::proton_page::ProtonPage;
use crate::ui::settings_page::SettingsPage;
use crate::ui::state::SharedState;

pub struct MainWindow {
    pub window: libadwaita::ApplicationWindow,
}

impl MainWindow {
    pub fn new(app: &libadwaita::Application, state: SharedState) -> Self {
        let window = libadwaita::ApplicationWindow::builder()
            .application(app)
            .title("Crucible")
            .default_width(1280)
            .default_height(800)
            .build();

        // Restore geometry
        {
            let st = state.lock().unwrap();
            if st.prefs.restore_geometry {
                window.set_default_size(st.prefs.window_width, st.prefs.window_height);
            }
        }

        // ── Layout: AdwNavigationSplitView (sidebar + content) ─────────────
        let split = libadwaita::NavigationSplitView::new();
        split.set_min_sidebar_width(200.0);
        split.set_max_sidebar_width(220.0);

        // ── Sidebar ────────────────────────────────────────────────────────
        let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        let sidebar_header = libadwaita::HeaderBar::new();
        sidebar_header.set_show_end_title_buttons(false);
        let title_label = gtk4::Label::new(Some("Crucible"));
        title_label.add_css_class("heading");
        sidebar_header.set_title_widget(Some(&title_label));
        sidebar_box.append(&sidebar_header);

        let nav_list = gtk4::ListBox::new();
        nav_list.set_selection_mode(gtk4::SelectionMode::Single);
        nav_list.add_css_class("navigation-sidebar");
        nav_list.set_vexpand(true);

        let rows = [
            ("view-grid-symbolic", "Library"),
            ("drive-harddisk-symbolic", "Runners"),
            ("preferences-system-symbolic", "Settings"),
        ];
        for (icon, label) in &rows {
            let row = libadwaita::ActionRow::new();
            row.set_title(label);
            let img = gtk4::Image::from_icon_name(icon);
            row.add_prefix(&img);
            nav_list.append(&row);
        }
        sidebar_box.append(&nav_list);

        let sidebar_page = libadwaita::NavigationPage::new(&sidebar_box, "sidebar");
        split.set_sidebar(Some(&sidebar_page));

        // ── Content stack ─────────────────────────────────────────────────
        let content_stack = gtk4::Stack::new();
        content_stack.set_transition_type(gtk4::StackTransitionType::Crossfade);

        let library = LibraryPage::new(state.clone());
        content_stack.add_named(&library.root, Some("library"));

        let runners = ProtonPage::new(state.clone());
        content_stack.add_named(&runners.root, Some("runners"));

        let settings = SettingsPage::new(state.clone());
        content_stack.add_named(&settings.root, Some("settings"));

        let content_header = libadwaita::HeaderBar::new();
        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content_box.append(&content_header);
        content_box.append(&content_stack);

        let content_page = libadwaita::NavigationPage::new(&content_box, "content");
        split.set_content(Some(&content_page));

        // Sidebar selection drives stack
        let stack = content_stack.clone();
        let pages = ["library", "runners", "settings"];
        nav_list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let idx = row.index() as usize;
                if let Some(name) = pages.get(idx) {
                    stack.set_visible_child_name(name);
                }
            }
        });
        // Select Library by default
        nav_list.select_row(nav_list.row_at_index(0).as_ref());

        window.set_content(Some(&split));

        // ── Save geometry on close ─────────────────────────────────────────
        let state_close = state.clone();
        window.connect_close_request(move |win| {
            let mut st = state_close.lock().unwrap();
            if st.prefs.restore_geometry {
                let (w, h) = (win.width(), win.height());
                st.prefs.window_width  = w;
                st.prefs.window_height = h;
                st.prefs.save();
            }
            glib::Propagation::Proceed
        });

        // ── Poll for game exits every second ───────────────────────────────
        let state_poll = state.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let mut st = state_poll.lock().unwrap();
            let exited = st.launcher.poll_exits();
            for name in exited {
                let secs = st.launcher.on_exited(&name);
                st.gm.record_playtime(&name, secs);
                log::info!("'{name}' exited after {secs}s");
            }
            glib::ControlFlow::Continue
        });

        Self { window }
    }

    pub fn present(&self) { self.window.present(); }
}
