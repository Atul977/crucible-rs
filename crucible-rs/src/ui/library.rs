// src/ui/library.rs — Game library page with cover grid, search, launch.

use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::core::game::GameConfig;
use crate::core::paths::display_name_from_exe;
use crate::core::shortcut::ShortcutManager;
use crate::ui::game_dialog::GameDialog;
use crate::ui::state::SharedState;

pub struct LibraryPage {
    pub root: gtk4::Widget,
}

impl LibraryPage {
    pub fn new(state: SharedState) -> Self {
        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // ── Toolbar ────────────────────────────────────────────────────────
        let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        toolbar.set_margin_top(12);
        toolbar.set_margin_bottom(8);
        toolbar.set_margin_start(16);
        toolbar.set_margin_end(16);

        let search_entry = gtk4::SearchEntry::new();
        search_entry.set_hexpand(true);
        search_entry.set_placeholder_text(Some("Search games…"));
        toolbar.append(&search_entry);

        let add_btn = gtk4::Button::new();
        add_btn.set_icon_name("list-add-symbolic");
        add_btn.add_css_class("suggested-action");
        add_btn.set_tooltip_text(Some("Add Game (or drag & drop an .exe)"));
        toolbar.append(&add_btn);

        vbox.append(&toolbar);
        vbox.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // ── Scroll + flow grid ─────────────────────────────────────────────
        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        let flow = gtk4::FlowBox::new();
        flow.set_selection_mode(gtk4::SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_row_spacing(12);
        flow.set_column_spacing(12);
        flow.set_margin_top(16);
        flow.set_margin_bottom(16);
        flow.set_margin_start(16);
        flow.set_margin_end(16);
        flow.set_max_children_per_line(10);
        flow.set_min_children_per_line(2);
        scroll.set_child(Some(&flow));
        vbox.append(&scroll);

        // ── Empty state ────────────────────────────────────────────────────
        let empty_page = libadwaita::StatusPage::new();
        empty_page.set_icon_name(Some("applications-games-symbolic"));
        empty_page.set_title("No Games Yet");
        empty_page.set_description(Some("Click + or drag an .exe file here to add your first game."));
        empty_page.set_vexpand(true);

        // Stack: either flow or empty page
        let content_stack = gtk4::Stack::new();
        content_stack.set_vexpand(true);
        content_stack.add_named(&scroll, Some("games"));
        content_stack.add_named(&empty_page, Some("empty"));
        // replace scroll with stack in vbox
        vbox.remove(&scroll);
        vbox.append(&content_stack);

        // ── Search filter ──────────────────────────────────────────────────
        let filter_text: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        {
            let filter_text = filter_text.clone();
            let flow = flow.clone();
            search_entry.connect_search_changed(move |e| {
                let query = e.text().to_string().to_lowercase();
                *filter_text.borrow_mut() = query.clone();
                // show/hide children
                let mut child = flow.first_child();
                while let Some(widget) = child {
                    let next = widget.next_sibling();
                    if let Some(label) = widget
                        .downcast_ref::<gtk4::FlowBoxChild>()
                        .and_then(|fbc| fbc.child())
                        .and_then(|c| c.downcast::<gtk4::Box>().ok())
                        .and_then(|b| {
                            let mut w = b.first_child();
                            while let Some(child) = w {
                                let next = child.next_sibling();
                                if let Some(lbl) = child.downcast_ref::<gtk4::Label>() {
                                    return Some(lbl.label().to_string());
                                }
                                w = next;
                            }
                            None
                        })
                    {
                        widget.set_visible(query.is_empty() || label.to_lowercase().contains(&query));
                    }
                    child = next;
                }
            });
        }

        // ── Populate function ──────────────────────────────────────────────
        let populate = {
            let flow = flow.clone();
            let state = state.clone();
            let content_stack = content_stack.clone();
            move || {
                // Remove all children
                while let Some(c) = flow.first_child() { flow.remove(&c); }

                let games: Vec<GameConfig> = {
                    let st = state.lock().unwrap();
                    st.gm.games_sorted().into_iter().cloned().collect()
                };

                if games.is_empty() {
                    content_stack.set_visible_child_name("empty");
                    return;
                }
                content_stack.set_visible_child_name("games");

                for game in games {
                    let card = make_game_card(&game, state.clone());
                    flow.insert(&card, -1);
                }
            }
        };

        // Initial population
        populate();

        // ── Add button → file chooser ──────────────────────────────────────
        {
            let state = state.clone();
            let populate = populate.clone();
            add_btn.connect_clicked(move |btn| {
                let dialog = gtk4::FileDialog::builder()
                    .title("Select game executable")
                    .build();
                let filter = gtk4::FileFilter::new();
                filter.add_pattern("*.exe");
                filter.set_name(Some("Windows executables (*.exe)"));
                let filters = gio::ListStore::new::<gtk4::FileFilter>();
                filters.append(&filter);
                dialog.set_filters(Some(&filters));

                let root = btn.root().and_downcast::<gtk4::Window>();
                let state = state.clone();
                let populate = populate.clone();
                dialog.open(root.as_ref(), None::<&gio::Cancellable>, move |result| {
                    let Ok(file) = result else { return };
                    let Some(path) = file.path() else { return };
                    let exe = path.to_string_lossy().into_owned();
                    let name = display_name_from_exe(&exe);
                    {
                        let mut st = state.lock().unwrap();
                        let proton = st.gm.global_config.proton_version.clone();
                        let extra = st.extra_proton_dirs.clone();
                        let _ = st.gm.add_game(
                            name, exe, proton, String::new(), String::new(),
                            String::new(), Default::default(), String::new(),
                            false, String::new(), "auto".into(),
                            false, false, false, Default::default(), &extra,
                        );
                    }
                    populate();
                });
            });
        }

        // ── Drag-and-drop .exe onto window ─────────────────────────────────
        let dnd_target = gtk4::DropTarget::new(gio::File::static_type(), gdk4::DragAction::COPY);
        {
            let state = state.clone();
            let populate = populate.clone();
            dnd_target.connect_drop(move |_, value, _, _| {
                if let Ok(file) = value.get::<gio::File>() {
                    if let Some(path) = file.path() {
                        if path.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                            let exe = path.to_string_lossy().into_owned();
                            let name = display_name_from_exe(&exe);
                            let mut st = state.lock().unwrap();
                            let proton = st.gm.global_config.proton_version.clone();
                            let extra = st.extra_proton_dirs.clone();
                            let _ = st.gm.add_game(
                                name, exe, proton, String::new(), String::new(),
                                String::new(), Default::default(), String::new(),
                                false, String::new(), "auto".into(),
                                false, false, false, Default::default(), &extra,
                            );
                            drop(st);
                            populate();
                            return true;
                        }
                    }
                }
                false
            });
        }
        vbox.add_controller(dnd_target);

        Self { root: vbox.upcast() }
    }
}

// ── Game card widget ───────────────────────────────────────────────────────

fn make_game_card(game: &GameConfig, state: SharedState) -> gtk4::Widget {
    let card_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    card_box.set_width_request(160);
    card_box.add_css_class("card");

    // Cover art
    let cover = gtk4::Picture::new();
    cover.set_width_request(160);
    cover.set_height_request(213); // ~3:4 ratio
    cover.set_content_fit(gtk4::ContentFit::Cover);
    cover.add_css_class("game-cover");

    // Load cover art asynchronously
    let cover_path = game.cover_path();
    let header_path = game.header_path();
    let cover_widget = cover.clone();
    glib::idle_add_local_once(move || {
        let path = if cover_path.exists() { cover_path }
                   else if header_path.exists() { header_path }
                   else { return };
        cover_widget.set_filename(Some(&path));
    });

    card_box.append(&cover);

    // Name label
    let name_label = gtk4::Label::new(Some(&game.name));
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name_label.set_max_width_chars(18);
    name_label.set_margin_top(6);
    name_label.set_margin_start(8);
    name_label.set_margin_end(8);
    name_label.add_css_class("caption");
    card_box.append(&name_label);

    // Playtime
    let playtime = game.playtime_display();
    if !playtime.is_empty() {
        let pt_label = gtk4::Label::new(Some(&playtime));
        pt_label.add_css_class("caption");
        pt_label.add_css_class("dim-label");
        pt_label.set_margin_bottom(8);
        card_box.append(&pt_label);
    } else {
        card_box.set_margin_bottom(8);
    }

    // ── Action buttons overlay ─────────────────────────────────────────────
    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&card_box));

    let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    btn_row.set_halign(gtk4::Align::Center);
    btn_row.set_valign(gtk4::Align::End);
    btn_row.set_margin_bottom(6);

    // Play button
    let play_btn = gtk4::Button::new();
    play_btn.set_icon_name("media-playback-start-symbolic");
    play_btn.add_css_class("circular");
    play_btn.add_css_class("suggested-action");
    play_btn.set_tooltip_text(Some("Launch"));
    btn_row.append(&play_btn);

    // Edit button
    let edit_btn = gtk4::Button::new();
    edit_btn.set_icon_name("document-edit-symbolic");
    edit_btn.add_css_class("circular");
    edit_btn.set_tooltip_text(Some("Settings"));
    btn_row.append(&edit_btn);

    // Stop button (shown when running)
    let stop_btn = gtk4::Button::new();
    stop_btn.set_icon_name("media-playback-stop-symbolic");
    stop_btn.add_css_class("circular");
    stop_btn.add_css_class("destructive-action");
    stop_btn.set_tooltip_text(Some("Stop"));
    stop_btn.set_visible(false);
    btn_row.append(&stop_btn);

    overlay.add_overlay(&btn_row);

    // ── Connect play ───────────────────────────────────────────────────────
    let game_name = game.name.clone();
    let state_play = state.clone();
    let play_btn2 = play_btn.clone();
    let stop_btn2 = stop_btn.clone();
    play_btn.connect_clicked(move |_| {
        let name = game_name.clone();
        let mut st = state_play.lock().unwrap();
        let st_ref = &mut *st;
        match st_ref.launcher.launch(&st_ref.gm, &name) {
            Ok(()) => {
                play_btn2.set_visible(false);
                stop_btn2.set_visible(true);
            }
            Err(e) => {
                drop(st);
                show_error_dialog(&name, &e.to_string());
            }
        }
    });

    // ── Connect stop ───────────────────────────────────────────────────────
    let game_name_stop = game.name.clone();
    let state_stop = state.clone();
    let play_btn3 = play_btn.clone();
    let stop_btn3 = stop_btn.clone();
    stop_btn.connect_clicked(move |_| {
        let mut st = state_stop.lock().unwrap();
        st.launcher.stop(&game_name_stop);
        play_btn3.set_visible(true);
        stop_btn3.set_visible(false);
    });

    // ── Connect edit ───────────────────────────────────────────────────────
    let game_name_edit = game.name.clone();
    let state_edit = state.clone();
    edit_btn.connect_clicked(move |btn| {
        let parent = btn.root().and_downcast::<gtk4::Window>();
        let dialog = GameDialog::new(parent.as_ref(), &game_name_edit, state_edit.clone());
        dialog.present();
    });

    // Make the card clickable (double-click → edit)
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(1);
    let game_name_click = game.name.clone();
    let state_click = state.clone();
    gesture.connect_released(move |g, n_press, _, _| {
        if n_press == 2 {
            let parent = g.widget().and_then(|w| w.root()).and_downcast::<gtk4::Window>();
            let dialog = GameDialog::new(parent.as_ref(), &game_name_click, state_click.clone());
            dialog.present();
        }
    });
    overlay.add_controller(gesture);

    overlay.upcast()
}

fn show_error_dialog(title: &str, msg: &str) {
    let dialog = libadwaita::AlertDialog::new(Some(title), Some(msg));
    dialog.add_response("ok", "OK");
    dialog.present(None::<&gtk4::Widget>);
}
