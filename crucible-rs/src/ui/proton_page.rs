// src/ui/proton_page.rs — Installed runners + download from GE/UMU/CachyOS.

use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::sync::{Arc, Mutex};

use crate::core::proton;
use crate::ui::state::SharedState;

pub struct ProtonPage {
    pub root: gtk4::Widget,
}

impl ProtonPage {
    pub fn new(state: SharedState) -> Self {
        let toolbar = libadwaita::ToolbarView::new();
        let header  = libadwaita::HeaderBar::new();

        // Source selector
        let source_model = gtk4::StringList::new(&["GE-Proton", "UMU-Proton", "CachyOS"]);
        let source_ids   = ["ge", "umu", "cachy"];
        let source_combo = gtk4::DropDown::new(Some(source_model), None::<gtk4::Expression>);
        source_combo.set_valign(gtk4::Align::Center);
        header.pack_end(&source_combo);

        toolbar.add_top_bar(&header);

        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // ── Progress bar (hidden when idle) ────────────────────────────────
        let progress = gtk4::ProgressBar::new();
        progress.set_show_text(true);
        progress.set_text(Some(""));
        progress.set_visible(false);
        progress.set_margin_start(16);
        progress.set_margin_end(16);
        progress.set_margin_top(8);
        outer.append(&progress);

        // ── Two-column paned: installed (left) | available (right) ─────────
        let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        paned.set_vexpand(true);

        // ── Left: installed runners ────────────────────────────────────────
        let inst_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let inst_label = gtk4::Label::new(Some("Installed"));
        inst_label.add_css_class("heading");
        inst_label.set_margin_top(12);
        inst_label.set_margin_bottom(8);
        inst_box.append(&inst_label);

        let inst_scroll = gtk4::ScrolledWindow::new();
        inst_scroll.set_vexpand(true);
        inst_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        let inst_list = gtk4::ListBox::new();
        inst_list.add_css_class("boxed-list");
        inst_list.set_selection_mode(gtk4::SelectionMode::None);
        inst_list.set_margin_start(8);
        inst_list.set_margin_end(8);
        inst_scroll.set_child(Some(&inst_list));
        inst_box.append(&inst_scroll);
        paned.set_start_child(Some(&inst_box));
        paned.set_position(300);

        // ── Right: available releases ──────────────────────────────────────
        let avail_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let avail_label = gtk4::Label::new(Some("Available"));
        avail_label.add_css_class("heading");
        avail_label.set_margin_top(12);
        avail_label.set_margin_bottom(8);
        avail_box.append(&avail_label);

        let avail_scroll = gtk4::ScrolledWindow::new();
        avail_scroll.set_vexpand(true);
        avail_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        let avail_list = gtk4::ListBox::new();
        avail_list.add_css_class("boxed-list");
        avail_list.set_selection_mode(gtk4::SelectionMode::None);
        avail_list.set_margin_start(8);
        avail_list.set_margin_end(8);

        let avail_spinner = gtk4::Spinner::new();
        avail_spinner.set_spinning(false);
        avail_spinner.set_halign(gtk4::Align::Center);
        avail_spinner.set_margin_top(32);
        avail_spinner.set_visible(false);

        let avail_inner = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        avail_inner.append(&avail_list);
        avail_inner.append(&avail_spinner);
        avail_scroll.set_child(Some(&avail_inner));
        avail_box.append(&avail_scroll);
        paned.set_end_child(Some(&avail_box));
        outer.append(&paned);
        toolbar.set_content(Some(&outer));

        // ── Populate installed list ────────────────────────────────────────
        let populate_installed = {
            let state = state.clone();
            let inst_list = inst_list.clone();
            move || {
                while let Some(c) = inst_list.first_child() { inst_list.remove(&c); }
                let st = state.lock().unwrap();
                let runners = st.gm.scan_proton(&st.extra_proton_dirs);
                if runners.is_empty() {
                    let row = libadwaita::ActionRow::new();
                    row.set_title("No runners installed");
                    row.set_subtitle("Download one from the right panel.");
                    inst_list.append(&row);
                    return;
                }
                for runner in runners {
                    let row = libadwaita::ActionRow::new();
                    row.set_title(&runner.name);
                    row.set_subtitle(&runner.version);
                    let del_btn = gtk4::Button::from_icon_name("user-trash-symbolic");
                    del_btn.add_css_class("flat");
                    del_btn.add_css_class("destructive-action");
                    del_btn.set_valign(gtk4::Align::Center);
                    del_btn.set_tooltip_text(Some("Delete"));
                    let tag = runner.name.clone();
                    let state2 = state.clone();
                    let inst_list2 = inst_list.clone();
                    del_btn.connect_clicked(move |btn| {
                        let alert = libadwaita::AlertDialog::new(
                            Some(&format!("Delete {tag}?")),
                            Some("This removes the Proton installation. Games using it will need a different version."),
                        );
                        alert.add_response("cancel", "Cancel");
                        alert.add_response("delete", "Delete");
                        alert.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
                        let tag2 = tag.clone();
                        let state3 = state2.clone();
                        let inst_list3 = inst_list2.clone();
                        alert.connect_response(None, move |_, resp| {
                            if resp == "delete" {
                                proton::delete_version(&tag2).ok();
                                while let Some(c) = inst_list3.first_child() { inst_list3.remove(&c); }
                                // repopulate
                                let st_lock = state3.lock().unwrap();
                                let runners2 = st_lock.gm.scan_proton(&st_lock.extra_proton_dirs);
                                drop(st_lock);
                                for r in &runners2 {
                                    let row2 = libadwaita::ActionRow::new();
                                    row2.set_title(&r.name);
                                    row2.set_subtitle(&r.version);
                                    inst_list3.append(&row2);
                                }
                            }
                        });
                        alert.present(Some(btn));
                    });
                    row.add_suffix(&del_btn);
                    inst_list.append(&row);
                }
            }
        };
        populate_installed();

        // ── Fetch available releases on source change ──────────────────────
        let fetch_releases = {
            let avail_list  = avail_list.clone();
            let avail_spinner = avail_spinner.clone();
            let state         = state.clone();
            let inst_list     = inst_list.clone();
            let progress      = progress.clone();
            let populate_inst = populate_installed.clone();
            move |source_idx: u32| {
                let source_id = source_ids.get(source_idx as usize).copied().unwrap_or("ge");
                while let Some(c) = avail_list.first_child() { avail_list.remove(&c); }
                avail_spinner.set_spinning(true);
                avail_spinner.set_visible(true);

                let avail_list2  = avail_list.clone();
                let avail_spin2  = avail_spinner.clone();
                let state2       = state.clone();
                let progress2    = progress.clone();
                let populate_i2  = populate_inst.clone();
                let source_str   = source_id.to_string();

                glib::MainContext::default().spawn_local(async move {
                    let extra = state2.lock().unwrap().extra_proton_dirs.clone();
                    let releases = proton::fetch_releases(&source_str, &extra).await
                        .unwrap_or_default();

                    avail_spin2.set_spinning(false);
                    avail_spin2.set_visible(false);
                    while let Some(c) = avail_list2.first_child() { avail_list2.remove(&c); }

                    if releases.is_empty() {
                        let row = libadwaita::ActionRow::new();
                        row.set_title("No releases found");
                        row.set_subtitle("Check your internet connection.");
                        avail_list2.append(&row);
                        return;
                    }

                    for rel in releases {
                        let row = libadwaita::ActionRow::new();
                        row.set_title(&rel.tag);
                        let size_mb = rel.size as f64 / 1_048_576.0;
                        row.set_subtitle(&if size_mb > 0.0 {
                            format!("{size_mb:.0} MB")
                        } else { String::new() });

                        if rel.installed {
                            let badge = gtk4::Label::new(Some("Installed"));
                            badge.add_css_class("success");
                            badge.set_valign(gtk4::Align::Center);
                            row.add_suffix(&badge);
                        } else {
                            let dl_btn = gtk4::Button::from_icon_name("folder-download-symbolic");
                            dl_btn.add_css_class("flat");
                            dl_btn.set_valign(gtk4::Align::Center);
                            dl_btn.set_tooltip_text(Some("Download and install"));

                            let rel_clone    = rel.clone();
                            let progress3    = progress2.clone();
                            let avail_list3  = avail_list2.clone();
                            let state3       = state2.clone();
                            let populate_i3  = populate_i2.clone();

                            dl_btn.connect_clicked(move |btn| {
                                btn.set_sensitive(false);
                                progress3.set_visible(true);
                                progress3.set_fraction(0.0);
                                progress3.set_text(Some("Starting…"));

                                let rel2 = rel_clone.clone();
                                let progress4 = progress3.clone();
                                let avail4    = avail_list3.clone();
                                let state4    = state3.clone();
                                let populate4 = populate_i3.clone();
                                let extra4    = state3.lock().unwrap().extra_proton_dirs.clone();

                                glib::MainContext::default().spawn_local(async move {
                                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(f32, String)>();

                                    let rel3   = rel2.clone();
                                    let extra5 = extra4.clone();
                                    let tx2    = tx.clone();

                                    let task = tokio::spawn(async move {
                                        proton::download_and_install(rel3, &extra5, move |f, msg| {
                                            let _ = tx2.send((f, msg.to_string()));
                                        }).await
                                    });

                                    while let Some((frac, msg)) = rx.recv().await {
                                        progress4.set_fraction(frac as f64);
                                        progress4.set_text(Some(&msg));
                                    }

                                    match task.await {
                                        Ok(Ok(())) => {
                                            progress4.set_fraction(1.0);
                                            progress4.set_text(Some("Installed ✓"));
                                            glib::timeout_add_local_once(
                                                std::time::Duration::from_secs(2),
                                                move || { progress4.set_visible(false); }
                                            );
                                            populate4();
                                        }
                                        _ => {
                                            progress4.set_text(Some("Download failed"));
                                        }
                                    }
                                });
                            });
                            row.add_suffix(&dl_btn);
                        }
                        avail_list2.append(&row);
                    }
                });
            }
        };

        // Trigger initial fetch for GE
        fetch_releases(0);

        // Re-fetch on source change
        source_combo.connect_selected_notify(move |combo| {
            fetch_releases(combo.selected());
        });

        Self { root: toolbar.upcast() }
    }
}
