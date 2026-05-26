// src/ui/settings_page.rs — App-wide preferences and global game defaults.

use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::ui::state::SharedState;

pub struct SettingsPage {
    pub root: gtk4::Widget,
}

impl SettingsPage {
    pub fn new(state: SharedState) -> Self {
        let toolbar = libadwaita::ToolbarView::new();
        let header  = libadwaita::HeaderBar::new();
        let save_btn = gtk4::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        header.pack_end(&save_btn);
        toolbar.add_top_bar(&header);

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scroll.set_vexpand(true);

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        vbox.set_margin_top(16);
        vbox.set_margin_bottom(16);
        vbox.set_margin_start(16);
        vbox.set_margin_end(16);
        scroll.set_child(Some(&vbox));
        toolbar.set_content(Some(&scroll));

        // ── App behaviour ──────────────────────────────────────────────────
        let app_group = libadwaita::PreferencesGroup::new();
        app_group.set_title("Application");

        let (prefs, gc) = {
            let st = state.lock().unwrap();
            (st.prefs.clone(), st.gm.global_config.clone())
        };

        let tray_row = switch_row("Minimize to Tray",  "Keep running in the system tray", prefs.minimize_to_tray);
        let geo_row  = switch_row("Restore Window Size","Remember window size on exit",    prefs.restore_geometry);
        let umu_row  = switch_row("Auto-update umu-run","Update silently on startup",      prefs.auto_update_umu);

        let log_levels = ["error", "warn", "info", "debug"];
        let log_idx = log_levels.iter().position(|&l| l == prefs.log_level).unwrap_or(2);
        let log_row = combo_row("Log Level", &log_levels, log_idx);

        app_group.add(&tray_row);
        app_group.add(&geo_row);
        app_group.add(&umu_row);
        app_group.add(&log_row);
        vbox.append(&app_group);

        // ── Paths ─────────────────────────────────────────────────────────
        let paths_group = libadwaita::PreferencesGroup::new();
        paths_group.set_title("Directories");

        let proton_dir_row = libadwaita::ActionRow::new();
        proton_dir_row.set_title("Custom Proton Directory");
        proton_dir_row.set_subtitle(if prefs.custom_proton_dir.is_empty() {
            "Not set (using ~/.steam/steam/compatibilitytools.d)"
        } else { &prefs.custom_proton_dir });
        proton_dir_row.set_subtitle_selectable(true);
        let browse_btn = gtk4::Button::from_icon_name("document-open-symbolic");
        browse_btn.add_css_class("flat");
        browse_btn.set_valign(gtk4::Align::Center);
        let clear_btn = gtk4::Button::from_icon_name("edit-clear-symbolic");
        clear_btn.add_css_class("flat");
        clear_btn.set_valign(gtk4::Align::Center);
        proton_dir_row.add_suffix(&browse_btn);
        proton_dir_row.add_suffix(&clear_btn);
        paths_group.add(&proton_dir_row);
        vbox.append(&paths_group);

        // browse custom proton dir
        {
            let row = proton_dir_row.clone();
            browse_btn.connect_clicked(move |btn| {
                let dialog = gtk4::FileDialog::builder()
                    .title("Select Proton directory").build();
                let root = btn.root().and_downcast::<gtk4::Window>();
                let row2 = row.clone();
                dialog.select_folder(root.as_ref(), None::<&gio::Cancellable>, move |res| {
                    if let Ok(f) = res {
                        if let Some(p) = f.path() {
                            row2.set_subtitle(&p.to_string_lossy());
                        }
                    }
                });
            });
        }
        {
            let row = proton_dir_row.clone();
            clear_btn.connect_clicked(move |_| {
                row.set_subtitle("Not set (using ~/.steam/steam/compatibilitytools.d)");
            });
        }

        // ── Global game defaults ───────────────────────────────────────────
        let gc_group = libadwaita::PreferencesGroup::new();
        gc_group.set_title("Global Game Defaults");
        gc_group.set_description(Some("Applied to all games unless overridden per-game."));

        let proton_names: Vec<String> = {
            let st = state.lock().unwrap();
            let extra = st.extra_proton_dirs.clone();
            st.gm.scan_proton(&extra).into_iter().map(|p| p.name).collect()
        };
        let proton_items: Vec<&str> = proton_names.iter().map(|s| s.as_str()).collect();
        let proton_idx = proton_names.iter()
            .position(|n| n == &gc.proton_version).unwrap_or(0);
        let def_proton_row = combo_row("Default Proton Version", &proton_items, proton_idx);

        let def_args_row    = entry_row("Default Launch Args",  &gc.launch_args);
        let def_wrapper_row = entry_row("Default Wrapper",      &gc.wrapper_command);
        let def_overrides   = entry_row("Default DLL Overrides",&gc.custom_overrides);

        let def_gamemode_row  = switch_row("GameMode by Default",  "", gc.enable_gamemode);
        let def_mangohud_row  = switch_row("MangoHud by Default",  "", gc.enable_mangohud);
        let def_gamescope_row = switch_row("Gamescope by Default", "", gc.enable_gamescope);
        let def_fp_row        = switch_row("Fingerprint Lock by Default", "", gc.fingerprint_lock);

        gc_group.add(&def_proton_row);
        gc_group.add(&def_args_row);
        gc_group.add(&def_wrapper_row);
        gc_group.add(&def_overrides);
        gc_group.add(&def_gamemode_row);
        gc_group.add(&def_mangohud_row);
        gc_group.add(&def_gamescope_row);
        gc_group.add(&def_fp_row);
        vbox.append(&gc_group);

        // ── Global env vars ────────────────────────────────────────────────
        let env_group = libadwaita::PreferencesGroup::new();
        env_group.set_title("Global Environment Variables");

        let env_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let env_entries: std::rc::Rc<std::cell::RefCell<Vec<(gtk4::Entry, gtk4::Entry)>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

        let add_env_row_fn = {
            let env_entries = env_entries.clone();
            let env_box = env_box.clone();
            move |key: &str, val: &str| {
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                let key_entry = gtk4::Entry::new();
                key_entry.set_placeholder_text(Some("KEY"));
                key_entry.set_text(key);
                key_entry.set_hexpand(true);
                let val_entry = gtk4::Entry::new();
                val_entry.set_placeholder_text(Some("value"));
                val_entry.set_text(val);
                val_entry.set_hexpand(true);
                let del_btn = gtk4::Button::from_icon_name("list-remove-symbolic");
                del_btn.add_css_class("flat");
                row.append(&key_entry);
                row.append(&gtk4::Label::new(Some("=")));
                row.append(&val_entry);
                row.append(&del_btn);

                let row_clone = row.clone();
                let ee = env_entries.clone();
                let ke = key_entry.clone();
                let ve = val_entry.clone();
                del_btn.connect_clicked(move |_| {
                    row_clone.set_visible(false);
                    ee.borrow_mut().retain(|(k, v)| k != &ke || v != &ve);
                });

                env_entries.borrow_mut().push((key_entry, val_entry));
                env_box.append(&row);
            }
        };

        let mut sorted_env: Vec<(String, String)> = gc.env_vars.iter()
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        sorted_env.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in &sorted_env {
            add_env_row_fn(k, v);
        }

        let add_env_btn = gtk4::Button::with_label("Add Variable");
        add_env_btn.add_css_class("flat");
        add_env_btn.set_icon_name("list-add-symbolic");
        let add_env_row_fn2 = add_env_row_fn.clone();
        add_env_btn.connect_clicked(move |_| { add_env_row_fn2("", ""); });

        let env_scroll_inner = gtk4::ScrolledWindow::new();
        env_scroll_inner.set_min_content_height(80);
        env_scroll_inner.set_max_content_height(180);
        env_scroll_inner.set_child(Some(&env_box));
        env_group.add(&env_scroll_inner);
        env_group.add(&add_env_btn);
        vbox.append(&env_group);

        // ── About ──────────────────────────────────────────────────────────
        let about_group = libadwaita::PreferencesGroup::new();
        about_group.set_title("About");
        let ver_row = libadwaita::ActionRow::new();
        ver_row.set_title("Crucible");
        ver_row.set_subtitle(&format!("v{} — Native Rust/GTK4 rewrite", env!("CARGO_PKG_VERSION")));
        about_group.add(&ver_row);
        vbox.append(&about_group);

        // ── Wire Save ─────────────────────────────────────────────────────
        let tray_r2        = tray_row.clone();
        let geo_r2         = geo_row.clone();
        let umu_r2         = umu_row.clone();
        let log_r2         = log_row.clone();
        let proton_dir_r2  = proton_dir_row.clone();
        let def_proton_r2  = def_proton_row.clone();
        let def_args_r2    = def_args_row.clone();
        let def_wrapper_r2 = def_wrapper_row.clone();
        let def_over_r2    = def_overrides.clone();
        let def_gm_r2      = def_gamemode_row.clone();
        let def_mh_r2      = def_mangohud_row.clone();
        let def_gs_r2      = def_gamescope_row.clone();
        let def_fp_r2      = def_fp_row.clone();
        let env_entries2   = env_entries.clone();
        let proton_names2  = proton_names.clone();
        let log_lvls       = log_levels.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        save_btn.connect_clicked(move |btn| {
            let mut st = state.lock().unwrap();

            // AppPrefs
            st.prefs.minimize_to_tray = tray_r2.is_active();
            st.prefs.restore_geometry = geo_r2.is_active();
            st.prefs.auto_update_umu  = umu_r2.is_active();
            st.prefs.log_level = log_lvls.get(log_r2.selected() as usize)
                .cloned().unwrap_or_else(|| "info".into());

            // Custom proton dir
            let new_dir = proton_dir_r2.subtitle()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let is_default = new_dir.starts_with("Not set");
            st.prefs.custom_proton_dir = if is_default { String::new() } else { new_dir.clone() };
            st.extra_proton_dirs = if is_default { vec![] } else { vec![PathBuf::from(&new_dir)] };
            st.launcher.set_extra_proton_dirs(st.extra_proton_dirs.clone());
            st.prefs.save();

            // GlobalConfig
            st.gm.global_config.proton_version = proton_names2
                .get(def_proton_r2.selected() as usize).cloned().unwrap_or_default();
            st.gm.global_config.launch_args     = def_args_r2.text().to_string();
            st.gm.global_config.wrapper_command  = def_wrapper_r2.text().to_string();
            st.gm.global_config.custom_overrides = def_over_r2.text().to_string();
            st.gm.global_config.enable_gamemode  = def_gm_r2.is_active();
            st.gm.global_config.enable_mangohud  = def_mh_r2.is_active();
            st.gm.global_config.enable_gamescope = def_gs_r2.is_active();
            st.gm.global_config.fingerprint_lock  = def_fp_r2.is_active();

            st.gm.global_config.env_vars = env_entries2.borrow().iter()
                .filter_map(|(k, v)| {
                    let kk = k.text().to_string();
                    if kk.is_empty() { None } else { Some((kk, v.text().to_string())) }
                })
                .collect();

            st.gm.global_config.save().ok();

            // Toast
            drop(st);
            show_toast(btn, "Settings saved");
        });

        Self { root: toolbar.upcast() }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn switch_row(title: &str, subtitle: &str, val: bool) -> libadwaita::SwitchRow {
    let r = libadwaita::SwitchRow::new();
    r.set_title(title);
    if !subtitle.is_empty() { r.set_subtitle(subtitle); }
    r.set_active(val);
    r
}

fn combo_row(title: &str, items: &[&str], active: usize) -> libadwaita::ComboRow {
    let r = libadwaita::ComboRow::new();
    r.set_title(title);
    r.set_model(Some(&gtk4::StringList::new(items)));
    r.set_selected(active as u32);
    r
}

fn entry_row(title: &str, val: &str) -> libadwaita::EntryRow {
    let r = libadwaita::EntryRow::new();
    r.set_title(title);
    r.set_text(val);
    r
}

fn show_toast(widget: &gtk4::Widget, msg: &str) {
    let Some(win) = widget.root().and_downcast::<libadwaita::ApplicationWindow>() else { return };
    if let Some(overlay) = find_toast_overlay(&win.upcast::<gtk4::Widget>()) {
        let toast = libadwaita::Toast::new(msg);
        overlay.add_toast(toast);
    }
}

fn find_toast_overlay(widget: &gtk4::Widget) -> Option<libadwaita::ToastOverlay> {
    let mut w: Option<gtk4::Widget> = Some(widget.clone());
    while let Some(cur) = w {
        if let Ok(overlay) = cur.clone().downcast::<libadwaita::ToastOverlay>() {
            return Some(overlay);
        }
        w = cur.first_child();
    }
    None
}
