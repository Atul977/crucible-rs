// src/ui/game_dialog.rs — Per-game settings dialog (General / Advanced / Info tabs).

use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::core::config::GamescopeSettings;
use crate::core::game::GameConfig;
use crate::core::paths::{display_name_from_exe, find_game_root};
use crate::core::shortcut::ShortcutManager;
use crate::core::steam::{fetch_artwork, SteamApi};
use crate::ui::state::SharedState;

pub struct GameDialog {
    pub dialog: libadwaita::Dialog,
}

impl GameDialog {
    pub fn new(parent: Option<&gtk4::Window>, game_name: &str, state: SharedState) -> Self {
        let dialog = libadwaita::Dialog::new();
        dialog.set_title(game_name);
	dialog.set_content_width(620);
        dialog.set_content_height(640);
        dialog.set_default_size(620, 640);
        dialog.set_can_close(true);
        dialog.set_focus_on_click(true);

        let toolbar_view = libadwaita::ToolbarView::new();
        let header = libadwaita::HeaderBar::new();
        toolbar_view.add_top_bar(&header);

        // ── Notebook (tabs) ────────────────────────────────────────────────
        let notebook = gtk4::Notebook::new();
        notebook.set_tab_pos(gtk4::PositionType::Top);

        let (general_tab, apply_general) = build_general_tab(game_name, state.clone());
        let (advanced_tab, apply_advanced) = build_advanced_tab(game_name, state.clone());
        let info_tab = build_info_tab(game_name, state.clone());

        notebook.append_page(&general_tab, Some(&gtk4::Label::new(Some("General"))));
        notebook.append_page(&advanced_tab, Some(&gtk4::Label::new(Some("Advanced"))));
        notebook.append_page(&info_tab,     Some(&gtk4::Label::new(Some("Info"))));

        // ── Apply / Cancel buttons ─────────────────────────────────────────
        let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        btn_row.set_halign(gtk4::Align::End);
        btn_row.set_margin_top(12);
        btn_row.set_margin_bottom(16);
        btn_row.set_margin_end(16);

        let cancel_btn = gtk4::Button::with_label("Cancel");
        let apply_btn  = gtk4::Button::with_label("Apply");
        apply_btn.add_css_class("suggested-action");
        btn_row.append(&cancel_btn);
        btn_row.append(&apply_btn);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.append(&notebook);
        content.append(&btn_row);
        toolbar_view.set_content(Some(&content));

        dialog.set_child(Some(&toolbar_view));

        // ── Wire up Cancel ─────────────────────────────────────────────────
        {
            let dialog = dialog.clone();
            cancel_btn.connect_clicked(move |_| { dialog.close(); });
        }

        // ── Wire up Apply ─────────────────────────────────────────────────
        {
            let dialog = dialog.clone();
            let game_name = game_name.to_string();
            let state_apply = state.clone();
            apply_btn.connect_clicked(move |_| {
                apply_general(&game_name, &state_apply);
                apply_advanced(&game_name, &state_apply);
                dialog.close();
            });
        }

        GameDialog { dialog }
    }

    pub fn present(&self) { self.dialog.present(None::<&gtk4::Widget>); }
}

// ── Helper: labelled row ───────────────────────────────────────────────────

fn pref_row(label: &str) -> (libadwaita::ActionRow, gtk4::Widget) {
    let row = libadwaita::ActionRow::new();
    row.set_title(label);
    (row, gtk4::Box::new(gtk4::Orientation::Horizontal, 0).upcast())
}

fn entry_row(title: &str, val: &str) -> libadwaita::EntryRow {
    let row = libadwaita::EntryRow::new();
    row.set_title(title);
    row.set_text(val);
    row
}

fn switch_row(title: &str, subtitle: &str, val: bool) -> libadwaita::SwitchRow {
    let row = libadwaita::SwitchRow::new();
    row.set_title(title);
    if !subtitle.is_empty() { row.set_subtitle(subtitle); }
    row.set_active(val);
    row
}

fn combo_row(title: &str, items: &[&str], active: usize) -> libadwaita::ComboRow {
    let row = libadwaita::ComboRow::new();
    row.set_title(title);
    let model = gtk4::StringList::new(items);
    row.set_model(Some(&model));
    row.set_selected(active as u32);
    row
}

fn section_box(title: &str) -> (gtk4::Box, libadwaita::PreferencesGroup) {
    let group = libadwaita::PreferencesGroup::new();
    group.set_title(title);
    let bx = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    bx.append(&group);
    (bx, group)
}

// ─────────────────────────────────────────────────────────────────────────────
// GENERAL TAB
// ─────────────────────────────────────────────────────────────────────────────

fn build_general_tab(
    game_name: &str,
    state: SharedState,
) -> (gtk4::Widget, Box<dyn Fn(&str, &SharedState)>) {

    let game = {
        let st = state.lock().unwrap();
        st.gm.get(game_name).cloned().unwrap_or_default()
    };
    let proton_names: Vec<String> = {
        let st = state.lock().unwrap();
        st.gm.scan_proton(&st.extra_proton_dirs)
            .into_iter().map(|p| p.name).collect()
    };

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_bottom(16);
    scroll.set_child(Some(&vbox));

    // ── Identity ───────────────────────────────────────────────────────────
    let id_group = libadwaita::PreferencesGroup::new();
    id_group.set_title("Game");

    let name_row = entry_row("Name", &game.name);
    id_group.add(&name_row);

    // Exe path
    let exe_row = libadwaita::ActionRow::new();
    exe_row.set_title("Executable");
    exe_row.set_subtitle(&game.exe_path);
    exe_row.set_subtitle_selectable(true);
    let browse_exe = gtk4::Button::from_icon_name("document-open-symbolic");
    browse_exe.add_css_class("flat");
    browse_exe.set_valign(gtk4::Align::Center);
    exe_row.add_suffix(&browse_exe);
    id_group.add(&exe_row);

    // Proton version dropdown
    let proton_items: Vec<&str> = proton_names.iter().map(|s| s.as_str()).collect();
    let proton_idx = proton_names.iter().position(|n| n == &game.proton_version).unwrap_or(0);
    let proton_row = combo_row("Proton Version", &proton_items, proton_idx);
    id_group.add(&proton_row);

    vbox.append(&id_group);

    // ── Launch ────────────────────────────────────────────────────────────
    let launch_group = libadwaita::PreferencesGroup::new();
    launch_group.set_title("Launch");
    let args_row    = entry_row("Launch arguments", &game.launch_args);
    let wrapper_row = entry_row("Wrapper command", &game.wrapper_command);
    launch_group.add(&args_row);
    launch_group.add(&wrapper_row);
    vbox.append(&launch_group);

    // ── Tools ─────────────────────────────────────────────────────────────
    let tools_group = libadwaita::PreferencesGroup::new();
    tools_group.set_title("Tools");
    let gamemode_row   = switch_row("GameMode",   "gamemoderun wrapper",   game.enable_gamemode);
    let mangohud_row   = switch_row("MangoHud",   "Performance overlay",   game.enable_mangohud);
    let gamescope_row  = switch_row("Gamescope",  "Embedded compositor",   game.enable_gamescope);
    let fp_row         = switch_row("Fingerprint Lock", "/proc isolation", game.fingerprint_lock);
    tools_group.add(&gamemode_row);
    tools_group.add(&mangohud_row);
    tools_group.add(&gamescope_row);
    tools_group.add(&fp_row);
    vbox.append(&tools_group);

    // ── Artwork ────────────────────────────────────────────────────────────
    let art_group = libadwaita::PreferencesGroup::new();
    art_group.set_title("Artwork");

    let fetch_art_btn = gtk4::Button::with_label("Fetch Steam Artwork");
    fetch_art_btn.add_css_class("pill");
    {
        let exe = game.exe_path.clone();
        let name = game.name.clone();
        fetch_art_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("Fetching…");
            let exe2 = exe.clone();
            let name2 = name.clone();
            let btn2 = btn.clone();
            glib::MainContext::default().spawn_local(async move {
                let api = SteamApi::new();
                fetch_artwork(&exe2, &name2, &api).await;
                btn2.set_sensitive(true);
                btn2.set_label("Done ✓");
            });
        });
    }
    art_group.add(&fetch_art_btn);

    let shortcut_btn_label = if ShortcutManager::has_shortcut(&game.name) {
        "Remove Desktop Shortcut"
    } else {
        "Create Desktop Shortcut"
    };
    let shortcut_btn = gtk4::Button::with_label(shortcut_btn_label);
    shortcut_btn.add_css_class("pill");
    {
        let gname = game.name.clone();
        let game_state = state.clone();
        let btn2 = shortcut_btn.clone();
        shortcut_btn.connect_clicked(move |_| {
            let st = game_state.lock().unwrap();
            if let Some(g) = st.gm.get(&gname) {
                if ShortcutManager::has_shortcut(&gname) {
                    ShortcutManager::remove(&gname);
                    btn2.set_label("Create Desktop Shortcut");
                } else {
                    ShortcutManager::create(g).ok();
                    btn2.set_label("Remove Desktop Shortcut");
                }
            }
        });
    }
    art_group.add(&shortcut_btn);
    vbox.append(&art_group);

    // ── Browse-exe wires up ───────────────────────────────────────────────
    let exe_row2 = exe_row.clone();
    browse_exe.connect_clicked(move |btn| {
        let root = btn.root().and_downcast::<gtk4::Window>();
        let filter = gtk4::FileFilter::new();
        filter.add_pattern("*.exe");
        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        let dlg = gtk4::FileDialog::builder().title("Select executable").build();
        dlg.set_filters(Some(&filters));
        let exe_row3 = exe_row2.clone();
        dlg.open(root.as_ref(), None::<&gio::Cancellable>, move |result| {
            if let Ok(f) = result {
                if let Some(p) = f.path() {
                    exe_row3.set_subtitle(&p.to_string_lossy());
                }
            }
        });
    });

    // ── Capture widget refs for apply closure ──────────────────────────────
    let name_row2    = name_row.clone();
    let exe_row3     = exe_row.clone();
    let proton_row2  = proton_row.clone();
    let args_row2    = args_row.clone();
    let wrapper_row2 = wrapper_row.clone();
    let gamemode_r2  = gamemode_row.clone();
    let mangohud_r2  = mangohud_row.clone();
    let gamescope_r2 = gamescope_row.clone();
    let fp_row2      = fp_row.clone();
    let proton_names2 = proton_names.clone();

    let apply = Box::new(move |name: &str, st: &SharedState| {
        let new_name     = name_row2.text().to_string();
        let new_exe      = exe_row3.subtitle().map(|s| s.to_string()).unwrap_or_default();
        let proton_idx   = proton_row2.selected() as usize;
        let proton_ver   = proton_names2.get(proton_idx).cloned().unwrap_or_default();
        let launch_args  = args_row2.text().to_string();
        let wrapper      = wrapper_row2.text().to_string();
        let gamemode     = gamemode_r2.is_active();
        let mangohud     = mangohud_r2.is_active();
        let gamescope    = gamescope_r2.is_active();
        let fp_lock      = fp_row2.is_active();

        let mut st_lock = st.lock().unwrap();
        let extra = st_lock.extra_proton_dirs.clone();

        // Rename if needed
        if !new_name.is_empty() && new_name != name {
            st_lock.gm.rename_game(name, &new_name).ok();
        }
        let target_name = if !new_name.is_empty() { new_name.clone() } else { name.to_string() };

        let _ = st_lock.gm.update_fields(&target_name, |g| {
            if !new_exe.is_empty() { g.exe_path = new_exe.clone(); }
            g.proton_version  = proton_ver.clone();
            g.launch_args     = launch_args.clone();
            g.wrapper_command = wrapper.clone();
            g.enable_gamemode  = gamemode;
            g.enable_mangohud  = mangohud;
            g.enable_gamescope = gamescope;
            g.fingerprint_lock = fp_lock;
        });
    });

    (scroll.upcast(), apply)
}

// ─────────────────────────────────────────────────────────────────────────────
// ADVANCED TAB
// ─────────────────────────────────────────────────────────────────────────────

fn build_advanced_tab(
    game_name: &str,
    state: SharedState,
) -> (gtk4::Widget, Box<dyn Fn(&str, &SharedState)>) {

    let game = {
        let st = state.lock().unwrap();
        st.gm.get(game_name).cloned().unwrap_or_default()
    };

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_bottom(16);
    scroll.set_child(Some(&vbox));

    // ── Wine ──────────────────────────────────────────────────────────────
    let wine_group = libadwaita::PreferencesGroup::new();
    wine_group.set_title("Wine");
    let overrides_row = entry_row("DLL Overrides", &game.custom_overrides);
    overrides_row.set_tooltip_text(Some(
        "Semi-colon separated: dll=mode. Mode: n,b b,n n b d  OR comma-separated DLL names."
    ));
    let prefix_row = entry_row("Wine Prefix", &game.prefix_path);
    wine_group.add(&overrides_row);
    wine_group.add(&prefix_row);
    vbox.append(&wine_group);

    // ── Environment variables ─────────────────────────────────────────────
    let env_group = libadwaita::PreferencesGroup::new();
    env_group.set_title("Environment Variables");
    env_group.set_description(Some("Key=Value pairs, one per row."));

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

    // Populate existing env vars
    let mut sorted_env: Vec<(String, String)> = game.env_vars.iter()
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
    env_scroll_inner.set_min_content_height(100);
    env_scroll_inner.set_max_content_height(200);
    env_scroll_inner.set_child(Some(&env_box));
    env_group.add(&env_scroll_inner);
    env_group.add(&add_env_btn);
    vbox.append(&env_group);

    // ── Gamescope settings (shown when gamescope enabled) ─────────────────
    let gs = &game.gamescope_settings;
    let gs_group = libadwaita::PreferencesGroup::new();
    gs_group.set_title("Gamescope");

    let gs_win_types = ["fullscreen", "borderless"];
    let gs_upscale   = ["none", "fsr", "nis", "integer", "stretch"];
    let gs_win_idx     = gs_win_types.iter().position(|&x| x == gs.window_type).unwrap_or(0);
    let gs_upscale_idx = gs_upscale.iter().position(|&x| x == gs.upscale_method).unwrap_or(0);

    let gs_win_row    = combo_row("Window Type", &gs_win_types, gs_win_idx);
    let gs_gw_row     = entry_row("Game Width",  &gs.game_width);
    let gs_gh_row     = entry_row("Game Height", &gs.game_height);
    let gs_uw_row     = entry_row("Output Width",  &gs.upscale_width);
    let gs_uh_row     = entry_row("Output Height", &gs.upscale_height);
    let gs_upsc_row   = combo_row("Upscale Method", &gs_upscale, gs_upscale_idx);
    let gs_fps_row    = entry_row("FPS Limit",       &gs.fps_limiter);
    let gs_fpsnf_row  = entry_row("FPS Limit (BG)",  &gs.fps_limiter_no_focus);
    let gs_cursor_row = switch_row("Force Grab Cursor", "", gs.enable_force_grab_cursor);
    let gs_extra_row  = entry_row("Extra Options",   &gs.additional_options);

    for r in [&gs_win_row, &gs_gw_row as &dyn IsA<gtk4::Widget>, &gs_gh_row, &gs_uw_row,
              &gs_uh_row, &gs_upsc_row, &gs_fps_row, &gs_fpsnf_row, &gs_extra_row] {
        // each is a libadwaita widget, so we add them to the group by trait dispatch
    }
    gs_group.add(&gs_win_row);
    gs_group.add(&gs_gw_row);
    gs_group.add(&gs_gh_row);
    gs_group.add(&gs_uw_row);
    gs_group.add(&gs_uh_row);
    gs_group.add(&gs_upsc_row);
    gs_group.add(&gs_fps_row);
    gs_group.add(&gs_fpsnf_row);
    gs_group.add(&gs_cursor_row);
    gs_group.add(&gs_extra_row);
    vbox.append(&gs_group);

    // ── Dangerous zone ─────────────────────────────────────────────────────
    let danger_group = libadwaita::PreferencesGroup::new();
    danger_group.set_title("Danger Zone");

    let reset_prefix_btn = gtk4::Button::with_label("Reset Wine Prefix");
    reset_prefix_btn.add_css_class("destructive-action");
    reset_prefix_btn.add_css_class("pill");
    {
        let gname = game_name.to_string();
        let st = state.clone();
        reset_prefix_btn.connect_clicked(move |btn| {
            let alert = libadwaita::AlertDialog::new(
                Some("Reset Wine Prefix?"),
                Some("This deletes all game saves and settings stored in the prefix. This cannot be undone."),
            );
            alert.add_response("cancel", "Cancel");
            alert.add_response("reset", "Reset");
            alert.set_response_appearance("reset", libadwaita::ResponseAppearance::Destructive);
            let gname2 = gname.clone();
            let st2 = st.clone();
            alert.connect_response(None, move |_, resp| {
                if resp == "reset" {
                    let st_lock = st2.lock().unwrap();
                    st_lock.gm.reset_prefix(&gname2);
                }
            });
            alert.present(Some(btn));
        });
    }

    let remove_game_btn = gtk4::Button::with_label("Remove Game");
    remove_game_btn.add_css_class("destructive-action");
    remove_game_btn.add_css_class("pill");
    {
        let gname = game_name.to_string();
        let st = state.clone();
        remove_game_btn.connect_clicked(move |btn| {
            let alert = libadwaita::AlertDialog::new(
                Some("Remove Game?"),
                Some("The game entry will be removed from Crucible. Game files are not deleted."),
            );
            alert.add_response("cancel", "Cancel");
            alert.add_response("remove", "Remove");
            alert.set_response_appearance("remove", libadwaita::ResponseAppearance::Destructive);
            let gname2 = gname.clone();
            let st2 = st.clone();
            alert.connect_response(None, move |_, resp| {
                if resp == "remove" {
                    let mut st_lock = st2.lock().unwrap();
                    st_lock.gm.remove_game(&gname2).ok();
                }
            });
            alert.present(Some(btn));
        });
    }

    danger_group.add(&reset_prefix_btn);
    danger_group.add(&remove_game_btn);
    vbox.append(&danger_group);

    // ── Apply closure ─────────────────────────────────────────────────────
    let overrides_r2 = overrides_row.clone();
    let prefix_r2    = prefix_row.clone();
    let env_entries2 = env_entries.clone();
    let gs_win_r2    = gs_win_row.clone();
    let gs_gw_r2     = gs_gw_row.clone();
    let gs_gh_r2     = gs_gh_row.clone();
    let gs_uw_r2     = gs_uw_row.clone();
    let gs_uh_r2     = gs_uh_row.clone();
    let gs_upsc_r2   = gs_upsc_row.clone();
    let gs_fps_r2    = gs_fps_row.clone();
    let gs_fpsnf_r2  = gs_fpsnf_row.clone();
    let gs_cursor_r2 = gs_cursor_row.clone();
    let gs_extra_r2  = gs_extra_row.clone();

    let win_types_str = gs_win_types.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let upscale_str   = gs_upscale.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    let apply = Box::new(move |name: &str, st: &SharedState| {
        let custom_overrides = overrides_r2.text().to_string();
        let prefix_path      = prefix_r2.text().to_string();

        let env_vars: HashMap<String, String> = env_entries2.borrow().iter()
            .filter_map(|(k, v)| {
                let kk = k.text().to_string();
                let vv = v.text().to_string();
                if kk.is_empty() { None } else { Some((kk, vv)) }
            })
            .collect();

        let gs = GamescopeSettings {
            window_type: win_types_str.get(gs_win_r2.selected() as usize)
                .cloned().unwrap_or_default(),
            game_width:    gs_gw_r2.text().to_string(),
            game_height:   gs_gh_r2.text().to_string(),
            upscale_width:  gs_uw_r2.text().to_string(),
            upscale_height: gs_uh_r2.text().to_string(),
            upscale_method: upscale_str.get(gs_upsc_r2.selected() as usize)
                .cloned().unwrap_or_default(),
            fps_limiter:          gs_fps_r2.text().to_string(),
            fps_limiter_no_focus: gs_fpsnf_r2.text().to_string(),
            enable_force_grab_cursor: gs_cursor_r2.is_active(),
            additional_options:   gs_extra_r2.text().to_string(),
        };

        let mut st_lock = st.lock().unwrap();
        let _ = st_lock.gm.update_fields(name, |g| {
            g.custom_overrides  = custom_overrides.clone();
            g.prefix_path       = prefix_path.clone();
            g.env_vars          = env_vars.clone();
            g.gamescope_settings = gs.clone();
        });
    });

    (scroll.upcast(), apply)
}

// ─────────────────────────────────────────────────────────────────────────────
// INFO TAB
// ─────────────────────────────────────────────────────────────────────────────

fn build_info_tab(game_name: &str, state: SharedState) -> gtk4::Widget {
    let game = {
        let st = state.lock().unwrap();
        st.gm.get(game_name).cloned().unwrap_or_default()
    };

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_bottom(16);
    scroll.set_child(Some(&vbox));

    let info_group = libadwaita::PreferencesGroup::new();
    info_group.set_title("Game Information");

    let make_info_row = |label: &str, value: &str| -> libadwaita::ActionRow {
        let r = libadwaita::ActionRow::new();
        r.set_title(label);
        r.set_subtitle(value);
        r.set_subtitle_selectable(true);
        r
    };

    info_group.add(&make_info_row("Executable",    &game.exe_path));
    info_group.add(&make_info_row("Install Dir",   &game.install_dir));
    info_group.add(&make_info_row("Wine Prefix",   &game.prefix_path));
    info_group.add(&make_info_row("Proton Version",&game.proton_version));
    info_group.add(&make_info_row("Playtime",      &game.playtime_display()));
    info_group.add(&make_info_row("Last Played",   &game.last_played));
    info_group.add(&make_info_row("Config File",   &game.game_file));
    vbox.append(&info_group);

    let log_group = libadwaita::PreferencesGroup::new();
    log_group.set_title("Logs");

    let open_logs_btn = gtk4::Button::with_label("Open Log Directory");
    open_logs_btn.add_css_class("pill");
    {
        let gname = game_name.to_string();
        open_logs_btn.connect_clicked(move |_| {
            let log_dir = crate::core::paths::Paths::game_logs_dir(&gname);
            let _ = std::process::Command::new("xdg-open")
                .arg(&log_dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        });
    }

    let clear_logs_btn = gtk4::Button::with_label("Clear Logs");
    clear_logs_btn.add_css_class("pill");
    {
        let gname = game_name.to_string();
        let st = state.clone();
        clear_logs_btn.connect_clicked(move |_| {
            let st_lock = st.lock().unwrap();
            st_lock.gm.clear_game_logs(&gname);
        });
    }

    log_group.add(&open_logs_btn);
    log_group.add(&clear_logs_btn);
    vbox.append(&log_group);

    scroll.upcast()
}
