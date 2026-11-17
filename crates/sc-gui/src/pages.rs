//! The four pages and the dialogs.

use crate::theme::*;
use crate::{App, Page, RowExt, Tone};
use eframe::egui::{self, Align, Id, Layout, RichText, TextEdit, Ui, Vec2};
use sc_core::attributes::Group;
use sc_core::joysticks::{FixPlan, Status};
use sc_core::launch::{self, LaunchItem};
use sc_core::{install, ops, profiles};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct RestoreDialog {
    pub name: String,
    pub controls: bool,
    pub user_cfg: bool,
    pub groups: BTreeSet<Group>,
    pub has_controls: bool,
    pub has_attributes: bool,
    pub has_user_cfg: bool,
}

fn js(n: Option<u32>) -> String {
    n.map_or("–".into(), |n| format!("js{n}"))
}

fn status_pill(ui: &mut Ui, status: Status) {
    match status {
        Status::Ok => pill(ui, "OK", GREEN),
        Status::Moved => pill(ui, "Moved", AMBER),
        Status::NotConnected => pill(ui, "Not connected", WEAK),
        Status::New => pill(ui, "New", ACCENT),
        Status::Unclear => pill(ui, "Unclear", RED),
    }
}

fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.heading(title);
    ui.label(weak(subtitle));
    ui.add_space(14.0);
}

/// A banner with an icon, a title, a line of explanation and buttons on the right.
fn banner(
    ui: &mut Ui,
    icon: &str,
    color: egui::Color32,
    title: &str,
    sub: &str,
    buttons: impl FnOnce(&mut Ui),
) {
    card(ui, |ui| {
        // The text wraps in the room the buttons leave. Their width is only
        // known once drawn, so it is remembered from the previous frame.
        let id = ui.scope_id().with(("banner", title));
        let buttons_width: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(240.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).color(color).size(26.0));
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - buttons_width - 24.0).max(160.0));
                ui.label(RichText::new(title).family(bold()).size(16.0));
                ui.label(weak(sub));
            });
            let used = ui
                .with_layout(Layout::right_to_left(Align::Center), |ui| {
                    buttons(ui);
                    ui.min_rect().width()
                })
                .inner;
            if (used - buttons_width).abs() > 1.0 {
                ui.data_mut(|d| d.insert_temp(id, used));
                ui.ctx().request_repaint();
            }
        });
    });
}

fn expand_home(s: &str) -> PathBuf {
    let s = s.trim().trim_matches('"');
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(s)
}

impl App {
    fn not_found(&mut self, ui: &mut Ui) {
        banner(
            ui,
            "⚠",
            AMBER,
            "Star Citizen was not found",
            "Add its folder in Settings to get started.",
            |ui| {
                if primary_button(ui, "Open Settings").clicked() {
                    self.page = Page::Settings;
                }
            },
        );
    }

    // ---------------------------------------------------------------- Joysticks

    pub fn joysticks_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Joysticks",
            "Keep every device's bindings on the device, whatever number the game gives it.",
        );
        let Some(ch) = self.channel.clone() else {
            self.not_found(ui);
            return;
        };
        if let Some(problem) = self.joy.problem.clone() {
            banner(ui, "⚠", RED, "Could not read your bindings", &problem, |ui| {
                if button(ui, "Refresh").clicked() {
                    self.reload_joysticks();
                }
            });
            ui.add_space(16.0);
        }
        let Some(live) = self.joy.live.clone() else { return };

        let Some(set) = self.joy.set.clone() else {
            banner(
                ui,
                "★",
                ACCENT,
                "Remember your joystick setup",
                "Do this once, while your bindings work. Then the tool can tell when the game renumbers your devices.",
                |ui| {
                    if primary_button(ui, "Remember current setup").clicked() {
                        self.remember(&ch);
                    }
                },
            );
            ui.add_space(16.0);
            card(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                egui::Grid::new("live").num_columns(3).spacing(Vec2::new(28.0, 14.0)).show(ui, |ui| {
                    for h in ["DEVICE", "NUMBER", "BINDINGS"] {
                        ui.label(weak(h).small().family(bold()));
                    }
                    ui.end_row();
                    for j in &live.joysticks {
                        device_cell(ui, &j.device);
                        ui.label(RichText::new(format!("js{}", j.instance)).monospace());
                        ui.label(live.bindings_on(j.instance).to_string());
                        ui.end_row();
                    }
                });
            });
            return;
        };

        let running = self.game_running();
        let moved = self.moved_count();
        let count =
            |s: Status| self.joy.rows.iter().filter(|r| r.effective_status(&self.overrides) == s).count();
        let (not_connected, unclear) = (count(Status::NotConnected), count(Status::Unclear));

        if moved > 0 {
            let title = if moved == 1 {
                "1 device has a new number".to_string()
            } else {
                format!("{moved} devices have new numbers")
            };
            banner(ui, "⚠", AMBER, &title, "Their bindings are still on the old numbers.", |ui| {
                let fix = ui.add_enabled_ui(!running, |ui| primary_button(ui, "Fix joysticks")).inner;
                let fix = fix
                    .on_disabled_hover_text("Close Star Citizen first: it rewrites this file when it exits.");
                if fix.clicked() {
                    self.fix(&ch);
                }
                if button(ui, "Preview changes").clicked() {
                    match ops::fix_joysticks(&self.data, &ch, &self.overrides, true) {
                        Ok(out) => self.preview = Some(out.plan),
                        Err(e) => self.say(format!("{e:#}"), Tone::Bad),
                    }
                }
            });
        } else {
            banner(
                ui,
                "✔",
                GREEN,
                "Every device's bindings are on the right number",
                &format!("Setup remembered {}.", set.saved_at),
                |ui| {
                    if button(ui, "Refresh").clicked() {
                        self.reload_joysticks();
                    }
                    if button(ui, "Remember current setup")
                        .on_hover_text("Use this after you change bindings or add a device.")
                        .clicked()
                    {
                        self.remember(&ch);
                    }
                },
            );
        }
        if not_connected > 0 || unclear > 0 {
            ui.add_space(6.0);
            if not_connected > 0 {
                ui.label(weak(format!(
                    "{not_connected} remembered device(s) are not connected. Their bindings are left alone, or moved out of the way if another device now has their number."
                )));
            }
            if unclear > 0 {
                ui.label(RichText::new(
                    "Some bindings could not be matched to a device (changed a lot since the setup was remembered, or two identical devices). Those are left alone. Remember the setup again once everything works.",
                ).color(AMBER));
            }
        }

        ui.add_space(16.0);
        let mut set_override: Option<(String, Option<u32>)> = None;
        card(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::Grid::new("devices").num_columns(5).spacing(Vec2::new(28.0, 14.0)).show(ui, |ui| {
                ui.label(weak("DEVICE").small().family(bold()));
                ui.label(weak("BINDINGS ON").small().family(bold()))
                    .on_hover_text("Where this device's bindings are in the game's file now");
                ui.label(weak("GAME USES NOW").small().family(bold()))
                    .on_hover_text("The number the game gives this device now. Pick one if it is wrong.");
                ui.label(weak("BINDINGS").small().family(bold()));
                ui.label(weak("STATUS").small().family(bold()));
                ui.end_row();
                for row in &self.joy.rows {
                    let key = row.device.key();
                    device_cell(ui, &row.device);
                    ui.label(RichText::new(js(row.bindings_on)).monospace().size(15.0));
                    let manual = self.overrides.get(&key).copied();
                    let now = manual.or(row.now);
                    let status = row.effective_status(&self.overrides);
                    let text = match manual {
                        Some(n) => RichText::new(format!("js{n} (set by you)")).color(AMBER),
                        None if status == Status::Moved => RichText::new(js(now)).color(AMBER),
                        None => RichText::new(js(now)),
                    };
                    egui::ComboBox::from_id_salt(("now", &key))
                        .selected_text(text.monospace())
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    manual.is_none(),
                                    format!("From the game ({})", js(row.now)),
                                )
                                .clicked()
                            {
                                set_override = Some((key.clone(), None));
                            }
                            for n in 1..=8 {
                                if ui.selectable_label(manual == Some(n), format!("js{n}")).clicked() {
                                    set_override = Some((key.clone(), Some(n)));
                                }
                            }
                        });
                    ui.label(row.bindings.to_string());
                    status_pill(ui, status);
                    ui.end_row();
                }
            });
        });
        if let Some((key, value)) = set_override {
            match value {
                Some(n) => self.overrides.insert(key, n),
                None => self.overrides.remove(&key),
            };
        }
        ui.add_space(10.0);
        ui.label(weak(
            "\"Game uses now\" comes from the game's own file. If it looks wrong, bind any button in the game's keybinding screen to see which jsN a stick really is, and pick that here.",
        ).small());
        ui.label(weak("A backup is made before every change. Star Citizen must be closed to apply.").small());
    }

    fn remember(&mut self, ch: &install::Channel) {
        match ops::remember(&self.data, ch) {
            Ok(set) => {
                self.say(format!("Remembered {} joysticks for {}.", set.devices.len(), ch.name), Tone::Good)
            }
            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
        }
        self.overrides.clear();
        self.reload_joysticks();
    }

    fn fix(&mut self, ch: &install::Channel) {
        match ops::fix_joysticks(&self.data, ch, &self.overrides, false) {
            Ok(out) if out.written => {
                let n = out.plan.moves.len() + out.plan.parked.len();
                self.say(
                    format!("Fixed: moved the bindings of {n} device(s). A backup was saved first."),
                    Tone::Good,
                );
                self.overrides.clear();
            }
            Ok(_) => self.say("Nothing to fix.", Tone::Info),
            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
        }
        self.preview = None;
        self.reload_joysticks();
    }

    // ---------------------------------------------------------------- Profiles

    pub fn profiles_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Profiles",
            "Save your settings and put them back after a patch, a reinstall, or on another game version.",
        );
        let Some(ch) = self.channel.clone() else {
            self.not_found(ui);
            return;
        };
        card(ui, |ui| {
            section_title(ui, &format!("Save {} settings", ch.name));
            ui.label(weak("Bindings with each device's saturation, deadzones, curves and inverts; graphics and other settings; and user.cfg."));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let edit = ui.add(
                    TextEdit::singleline(&mut self.new_profile_name)
                        .hint_text("Profile name, e.g. Gunfighters")
                        .desired_width(280.0)
                        .margin(Vec2::new(10.0, 8.0)),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (primary_button(ui, "Save").clicked() || enter) && !self.new_profile_name.trim().is_empty()
                {
                    let name = self.new_profile_name.trim().to_string();
                    if profiles::find(&self.data, &name).is_ok() {
                        self.confirm_replace = Some(name);
                    } else {
                        self.save_profile(&name, false);
                    }
                }
            });
        });
        ui.add_space(16.0);
        if self.profiles.is_empty() {
            ui.label(weak("No profiles yet."));
            return;
        }
        let list = self.profiles.clone();
        for p in list {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(p.name()).family(bold()).size(16.0));
                        ui.label(
                            weak(format!("Saved {} from {}", p.meta.created, p.meta.from_channel)).small(),
                        );
                        ui.horizontal(|ui| {
                            if p.has_controls() {
                                pill(ui, "Controls", ACCENT)
                            }
                            if p.has_attributes() {
                                pill(ui, "Graphics & settings", ACCENT)
                            }
                            if p.has_user_cfg() {
                                pill(ui, "user.cfg", ACCENT)
                            }
                        });
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, "Delete").clicked() {
                            self.confirm_delete = Some(p.name().to_string());
                        }
                        if primary_button(ui, "Restore…").clicked() {
                            self.restore = Some(RestoreDialog {
                                name: p.name().to_string(),
                                controls: p.has_controls(),
                                user_cfg: p.has_user_cfg(),
                                groups: Group::DEFAULT.into_iter().collect(),
                                has_controls: p.has_controls(),
                                has_attributes: p.has_attributes(),
                                has_user_cfg: p.has_user_cfg(),
                            });
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }
    }

    fn save_profile(&mut self, name: &str, overwrite: bool) {
        let Some(ch) = self.channel.clone() else { return };
        match profiles::save(&self.data, name, &ch, overwrite) {
            Ok(p) => {
                self.say(format!("Saved \"{}\" from {}.", p.name(), ch.name), Tone::Good);
                self.new_profile_name.clear();
            }
            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
        }
        self.reload_profiles();
    }

    // ---------------------------------------------------------------- Launch with

    pub fn launch_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Launch with",
            "Programs to start alongside the game, like TrackIR, JoyToKey or OBS.",
        );
        let enabled = self.config.launch.iter().filter(|i| i.enabled).count();
        ui.horizontal(|ui| {
            let all = ui
                .add_enabled_ui(enabled > 0, |ui| primary_button(ui, &format!("▶  Launch all ({enabled})")))
                .inner;
            if all.clicked() {
                let failed = launch::launch_all(&self.config.launch);
                if failed.is_empty() {
                    self.say(format!("Started {enabled} program(s)."), Tone::Good);
                } else {
                    let names: Vec<String> = failed.iter().map(|(n, e)| format!("{n}: {e}")).collect();
                    self.say(format!("Some did not start. {}", names.join("; ")), Tone::Bad);
                }
            }
            if button(ui, "Add program…").clicked()
                && let Some(path) = rfd::FileDialog::new().set_title("Choose a program to launch").pick_file()
            {
                self.config.launch.push(LaunchItem::new(&path.to_string_lossy()));
                self.save_config();
            }
            if button(ui, "Add link or command").clicked() {
                self.config.launch.push(LaunchItem {
                    name: "New item".into(),
                    target: String::new(),
                    args: String::new(),
                    enabled: true,
                });
                self.save_config();
            }
        });
        ui.add_space(4.0);
        ui.label(weak(if cfg!(windows) {
            "Programs, shortcuts (.lnk) and links like steam://… all work. Add the RSI Launcher too and one click starts everything."
        } else {
            "Programs, scripts and links work. For the game itself, add your Lutris or Steam link, or the LUG helper's launch script."
        }).small());
        ui.add_space(12.0);

        let mut remove = None;
        let mut start = None;
        let mut changed = false;
        for (i, item) in self.config.launch.iter_mut().enumerate() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    changed |=
                        ui.checkbox(&mut item.enabled, "").on_hover_text("Include in Launch all").changed();
                    changed |= ui
                        .add(
                            TextEdit::singleline(&mut item.name)
                                .desired_width(220.0)
                                .font(egui::FontId::new(15.0, bold())),
                        )
                        .changed();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✕").on_hover_text("Remove").clicked() {
                            remove = Some(i);
                        }
                        if ui.button("▶ Start").clicked() {
                            start = Some(i);
                        }
                    });
                });
                egui::Grid::new(("launch", i)).num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
                    ui.label(weak("Program"));
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                TextEdit::singleline(&mut item.target)
                                    .hint_text("Path, shortcut or link")
                                    .code_editor()
                                    .desired_width(ui.available_width() - 100.0),
                            )
                            .changed();
                        if ui.button("Browse…").clicked()
                            && let Some(p) = rfd::FileDialog::new().pick_file()
                        {
                            item.target = p.to_string_lossy().into_owned();
                            changed = true;
                        }
                    });
                    ui.end_row();
                    ui.label(weak("Arguments"));
                    changed |= ui
                        .add(
                            TextEdit::singleline(&mut item.args)
                                .hint_text("Optional")
                                .code_editor()
                                .desired_width(ui.available_width()),
                        )
                        .changed();
                    ui.end_row();
                });
            });
            ui.add_space(10.0);
        }
        if changed {
            self.config_changed();
        }
        if let Some(i) = start {
            let item = self.config.launch[i].clone();
            match launch::launch(&item) {
                Ok(()) => self.say(format!("Started {}.", item.name), Tone::Good),
                Err(e) => self.say(format!("{e:#}"), Tone::Bad),
            }
        }
        if let Some(i) = remove {
            self.config.launch.remove(i);
            self.save_config();
        }
        if self.config.launch.is_empty() {
            ui.label(weak("Nothing added yet."));
        }
    }

    // ---------------------------------------------------------------- Settings

    pub fn settings_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Settings", "Where Star Citizen is, and where this tool keeps its own files.");
        let mut remove = None;
        let mut add: Option<PathBuf> = None;
        card(ui, |ui| {
            section_title(ui, "Star Citizen");
            if self.installs.is_empty() {
                ui.label(RichText::new("Not found automatically. Add its folder below.").color(AMBER));
            }
            for root in self.installs.clone() {
                let manual = self.config.installs.contains(&root);
                ui.horizontal(|ui| {
                    let room = ui.available_width() - if manual { 110.0 } else { 0.0 };
                    ui.vertical(|ui| {
                        ui.set_max_width(room);
                        path_label(ui, &root);
                        let versions: Vec<String> =
                            install::channels(&root).into_iter().map(|c| c.name).collect();
                        ui.label(
                            weak(format!(
                                "{} · {}",
                                versions.join(", "),
                                if manual { "added by you" } else { "found automatically" }
                            ))
                            .small(),
                        );
                    });
                    if manual {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                remove = Some(root.clone());
                            }
                        });
                    }
                });
                ui.add_space(4.0);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if button(ui, "Add folder…").clicked()
                    && let Some(p) =
                        rfd::FileDialog::new().set_title("Choose your StarCitizen folder").pick_folder()
                {
                    add = Some(p);
                }
                ui.add(
                    TextEdit::singleline(&mut self.add_path)
                        .hint_text("…or paste a folder path")
                        .desired_width(380.0)
                        .margin(Vec2::new(10.0, 8.0)),
                );
                if button(ui, "Add").clicked() && !self.add_path.trim().is_empty() {
                    add = Some(expand_home(&self.add_path));
                }
                if button(ui, "Search again").clicked() {
                    self.rescan();
                    self.say(format!("Found {} Star Citizen folder(s).", self.installs.len()), Tone::Info);
                }
            });
            ui.label(weak(if cfg!(windows) {
                "Pick the StarCitizen folder, a version folder such as LIVE, or the Roberts Space Industries folder."
            } else {
                "On Linux it is inside your Wine prefix, e.g. ~/Games/star-citizen/drive_c/Program Files/Roberts Space Industries."
            }).small());
        });
        if let Some(root) = remove {
            self.config.installs.retain(|p| p != &root);
            self.save_config();
            self.rescan();
        }
        if let Some(picked) = add {
            match install::normalise_root(&picked) {
                Some(root) => {
                    if !self.config.installs.contains(&root) {
                        self.config.installs.push(root.clone());
                    }
                    self.add_path.clear();
                    self.save_config();
                    self.rescan();
                    if let Some(first) = install::channels(&root).into_iter().next() {
                        self.select(first);
                    }
                    self.say(format!("Added {}.", root.display()), Tone::Good);
                }
                None => self
                    .say(format!("No Star Citizen game versions found in {}.", picked.display()), Tone::Bad),
            }
        }

        ui.add_space(16.0);
        card(ui, |ui| {
            section_title(ui, "Your data");
            path_label(ui, &self.data.root);
            ui.label(weak(if self.data.portable {
                "Portable: profiles, remembered joysticks, backups and settings live in this folder next to the program. Move or copy the whole folder and they come with it."
            } else {
                "Kept in your user profile. Put a file called portable.txt next to the program to keep everything beside it instead."
            }));
            ui.label(weak("Backups of the game's files are made before every change; the newest 40 are kept in the backups folder."));
        });

        ui.add_space(16.0);
        card(ui, |ui| {
            section_title(ui, "About");
            ui.label(format!("SC All-In-One {}", env!("CARGO_PKG_VERSION")));
            ui.label(weak("Free software under the GNU General Public License, version 3 or later."));
            ui.hyperlink_to("Source code and releases", env!("CARGO_PKG_REPOSITORY"));
            ui.label(weak(
                "Not affiliated with Cloud Imperium Games or Roberts Space Industries. It only edits the settings files the game's own options menus write.",
            ).small());
        });
    }

    // ---------------------------------------------------------------- Dialogs

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(plan) = self.preview.clone() {
            let resp = egui::Modal::new(Id::unique("preview")).show(ctx, |ui| {
                ui.set_width(480.0);
                section_title(ui, "Changes the fix will make");
                ui.add_space(6.0);
                preview_lines(ui, &plan);
                ui.add_space(12.0);
                let mut close = false;
                ui.horizontal(|ui| {
                    let running = self.game_running();
                    let apply = ui.add_enabled_ui(!running, |ui| primary_button(ui, "Fix joysticks")).inner;
                    if apply.on_disabled_hover_text("Close Star Citizen first.").clicked() {
                        if let Some(ch) = self.channel.clone() {
                            self.fix(&ch);
                        }
                        close = true;
                    }
                    if button(ui, "Close").clicked() {
                        close = true;
                    }
                });
                close
            });
            if resp.inner || resp.should_close() {
                self.preview = None;
            }
        }

        if let Some(name) = self.confirm_delete.clone() {
            let resp = egui::Modal::new(Id::unique("delete")).show(ctx, |ui| {
                section_title(ui, &format!("Delete \"{name}\"?"));
                ui.label(weak("The saved profile is removed. Your game settings are not touched."));
                ui.add_space(10.0);
                let mut done = false;
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Delete").color(ON_ACCENT).family(bold()))
                                .fill(RED)
                                .min_size(Vec2::new(0.0, 36.0)),
                        )
                        .clicked()
                    {
                        match profiles::delete(&self.data, &name) {
                            Ok(()) => self.say(format!("Deleted \"{name}\"."), Tone::Info),
                            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
                        }
                        self.reload_profiles();
                        done = true;
                    }
                    if button(ui, "Cancel").clicked() {
                        done = true;
                    }
                });
                done
            });
            if resp.inner || resp.should_close() {
                self.confirm_delete = None;
            }
        }

        if let Some(name) = self.confirm_replace.clone() {
            let resp = egui::Modal::new(Id::unique("replace")).show(ctx, |ui| {
                section_title(ui, &format!("Replace \"{name}\"?"));
                ui.label(weak("A profile with this name exists. Replace it with the current settings?"));
                ui.add_space(10.0);
                let mut done = false;
                ui.horizontal(|ui| {
                    if primary_button(ui, "Replace").clicked() {
                        self.save_profile(&name, true);
                        done = true;
                    }
                    if button(ui, "Cancel").clicked() {
                        done = true;
                    }
                });
                done
            });
            if resp.inner || resp.should_close() {
                self.confirm_replace = None;
            }
        }

        self.restore_dialog(ctx);
    }

    fn restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.restore.take() else { return };
        let Some(ch) = self.channel.clone() else { return };
        let running = self.game_running();
        let mut keep_open = true;
        let resp = egui::Modal::new(Id::unique("restore")).show(ctx, |ui| {
            ui.set_width(520.0);
            section_title(ui, &format!("Restore \"{}\" into {}", d.name, ch.name));
            ui.add_space(6.0);
            if d.has_controls {
                ui.checkbox(&mut d.controls, RichText::new("Controls").family(bold()));
                ui.indent("c", |ui| {
                    ui.label(weak("Bindings, saturation, deadzones, curves and inverts. Moved to the numbers your devices have today.").small());
                });
            }
            if d.has_user_cfg {
                ui.checkbox(&mut d.user_cfg, RichText::new("user.cfg").family(bold()));
            }
            if d.has_attributes {
                ui.add_space(6.0);
                ui.label(RichText::new("Settings").family(bold()));
                for g in Group::ALL {
                    let mut on = d.groups.contains(&g);
                    if ui.checkbox(&mut on, g.label()).on_hover_text(g.description()).changed() {
                        if on { d.groups.insert(g); } else { d.groups.remove(&g); }
                    }
                }
            }
            ui.add_space(8.0);
            ui.label(weak("Anything not ticked is left as it is. A backup is made first.").small());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let anything = (d.controls && d.has_controls) || (d.user_cfg && d.has_user_cfg) || (!d.groups.is_empty() && d.has_attributes);
                let go = ui.add_enabled_ui(anything && !running, |ui| primary_button(ui, "Restore")).inner;
                if go.on_disabled_hover_text(if running { "Close Star Citizen first." } else { "Tick something to restore." }).clicked() {
                    self.do_restore(&d, &ch);
                    keep_open = false;
                }
                if button(ui, "Cancel").clicked() {
                    keep_open = false;
                }
            });
        });
        if keep_open && !resp.should_close() {
            self.restore = Some(d);
        }
    }

    fn do_restore(&mut self, d: &RestoreDialog, ch: &install::Channel) {
        let result = profiles::find(&self.data, &d.name).and_then(|p| {
            let opts =
                ops::RestoreOptions { controls: d.controls, user_cfg: d.user_cfg, groups: d.groups.clone() };
            ops::restore_profile(&self.data, &p, ch, &opts)
        });
        match result {
            Ok(out) => {
                let mut parts = Vec::new();
                if let Some(plan) = &out.controls {
                    parts.push(match plan.moves.len() {
                        0 => "controls".to_string(),
                        n => format!("controls ({n} device(s) moved to today's numbers)"),
                    });
                }
                if let Some(s) = &out.attributes {
                    parts.push(format!("{} setting(s) changed", s.changed + s.added));
                }
                if out.user_cfg {
                    parts.push("user.cfg".into());
                }
                self.say(
                    format!("Restored \"{}\": {}. A backup was saved first.", d.name, parts.join(", ")),
                    Tone::Good,
                );
            }
            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
        }
        self.overrides.clear();
        self.reload_joysticks();
    }
}

/// A path on one line, shortened with "…" if it does not fit; the full path
/// shows on hover.
fn path_label(ui: &mut Ui, path: &std::path::Path) {
    let text = path.display().to_string();
    ui.add(egui::Label::new(RichText::new(&text).monospace()).truncate()).on_hover_text(text);
}

fn device_cell(ui: &mut Ui, device: &sc_core::device::DeviceId) {
    ui.vertical(|ui| {
        ui.label(RichText::new(&device.name).family(bold()));
        ui.label(weak(device.hardware_label()).small().monospace());
    });
}

fn preview_lines(ui: &mut Ui, plan: &FixPlan) {
    if plan.is_noop() {
        ui.label("Nothing to change.");
        return;
    }
    for (device, from, to) in &plan.moves {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&device.name).family(bold()));
            ui.label(weak(format!("bindings js{from} → js{to}")));
        });
    }
    for (label, from, to) in &plan.parked {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).family(bold()));
            ui.label(weak(format!("not connected: js{from} → js{to}, out of the way")));
        });
    }
}
