// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! SC All-In-One: save and restore Star Citizen settings profiles, keep
//! joystick bindings on the right devices, and start companion apps.

mod pages;
mod theme;

use eframe::egui::{self, Align, Color32, Frame, Layout, Margin, RichText, Stroke};
use sc_core::data::{Config, DataDir};
use sc_core::install::{self, Channel};
use sc_core::joysticks::Status;
use sc_core::ops::{self, JoystickState};
use sc_core::profiles::{self, Profile};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};
use theme::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Joysticks,
    Profiles,
    Launch,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Bad,
    Info,
}

pub struct App {
    pub data: DataDir,
    pub config: Config,
    pub installs: Vec<PathBuf>,
    pub channel: Option<Channel>,
    pub page: Page,

    pub joy: JoystickState,
    /// "Game uses now" set by hand, by device key.
    pub overrides: BTreeMap<String, u32>,
    pub preview: Option<sc_core::joysticks::FixPlan>,
    actionmaps_mtime: Option<SystemTime>,
    last_file_check: Instant,

    pub profiles: Vec<Profile>,
    pub new_profile_name: String,
    pub restore: Option<pages::RestoreDialog>,
    pub confirm_delete: Option<String>,
    pub confirm_replace: Option<String>,

    pub add_path: String,
    config_dirty: Option<Instant>,

    game_running: Arc<AtomicBool>,
    message: Option<(String, Tone, Instant)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let (data, startup_error) = match DataDir::locate() {
            Ok(d) => (d, None),
            Err(e) => (DataDir::at("data"), Some(format!("{e:#}"))),
        };
        let (config, config_error) = match Config::load(&data) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("{e:#}"))),
        };

        // Watch for the game in the background; checking every process takes
        // long enough that it should not hold up drawing.
        let game_running = Arc::new(AtomicBool::new(false));
        {
            let flag = game_running.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                loop {
                    let now = sc_core::game::is_running();
                    if flag.swap(now, Ordering::Relaxed) != now {
                        ctx.request_repaint();
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            });
        }

        let mut app = App {
            data,
            config,
            installs: Vec::new(),
            channel: None,
            page: Page::Joysticks,
            joy: JoystickState::default(),
            overrides: BTreeMap::new(),
            preview: None,
            actionmaps_mtime: None,
            last_file_check: Instant::now(),
            profiles: Vec::new(),
            new_profile_name: String::new(),
            restore: None,
            confirm_delete: None,
            confirm_replace: None,
            add_path: String::new(),
            config_dirty: None,
            game_running,
            message: None,
        };
        app.rescan();
        app.reload_profiles();
        if let Some(e) = startup_error.or(config_error) {
            app.say(e, Tone::Bad);
        }
        app
    }

    pub fn game_running(&self) -> bool {
        self.game_running.load(Ordering::Relaxed)
    }

    pub fn say(&mut self, text: impl Into<String>, tone: Tone) {
        self.message = Some((text.into(), tone, Instant::now()));
    }

    /// Find installs again and keep the selected game version if it is still there.
    pub fn rescan(&mut self) {
        self.installs = ops::installs(&self.config);
        let current = self.channel.as_ref().map(|c| c.dir.clone());
        let chosen = match current {
            Some(dir) => self.all_channels().into_iter().find(|c| c.dir == dir),
            None => ops::select_channel(&self.config, None, None).ok(),
        };
        self.channel = chosen.or_else(|| self.all_channels().into_iter().next());
        self.reload_joysticks();
    }

    pub fn all_channels(&self) -> Vec<Channel> {
        self.installs.iter().flat_map(|root| install::channels(root)).collect()
    }

    pub fn select(&mut self, channel: Channel) {
        self.config.selected_install = channel.dir.parent().map(|p| p.to_path_buf());
        self.config.selected_channel = Some(channel.name.clone());
        self.channel = Some(channel);
        self.save_config();
        self.overrides.clear();
        self.reload_joysticks();
    }

    pub fn reload_joysticks(&mut self) {
        match &self.channel {
            Some(ch) => {
                self.joy = ops::joystick_state(&self.data, ch);
                self.actionmaps_mtime = std::fs::metadata(ch.actionmaps()).and_then(|m| m.modified()).ok();
            }
            None => {
                self.joy = JoystickState::default();
                self.actionmaps_mtime = None;
            }
        }
    }

    pub fn reload_profiles(&mut self) {
        match profiles::list(&self.data) {
            Ok(p) => self.profiles = p,
            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
        }
    }

    pub fn save_config(&mut self) {
        self.config_dirty = None;
        if let Err(e) = self.config.save(&self.data) {
            self.say(format!("Could not save settings: {e:#}"), Tone::Bad);
        }
    }

    /// Save a little after the last keystroke rather than on every one.
    pub fn config_changed(&mut self) {
        self.config_dirty = Some(Instant::now());
    }

    fn housekeeping(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.config_dirty {
            if t.elapsed() > Duration::from_millis(800) {
                self.save_config();
            } else {
                ctx.request_repaint_after(Duration::from_millis(900));
            }
        }
        // Pick up the game rewriting its bindings file, e.g. when it exits.
        if self.last_file_check.elapsed() > Duration::from_secs(2) {
            self.last_file_check = Instant::now();
            if let Some(ch) = &self.channel {
                let mtime = std::fs::metadata(ch.actionmaps()).and_then(|m| m.modified()).ok();
                if mtime != self.actionmaps_mtime {
                    self.reload_joysticks();
                }
            }
        }
        ctx.request_repaint_after(Duration::from_secs(2));
    }

    pub fn moved_count(&self) -> usize {
        self.joy.rows.iter().filter(|r| r.effective_status(&self.overrides) == Status::Moved).count()
    }
}

/// Status with any number set by hand applied.
pub trait RowExt {
    fn effective_status(&self, overrides: &BTreeMap<String, u32>) -> Status;
}

impl RowExt for sc_core::joysticks::DeviceRow {
    fn effective_status(&self, overrides: &BTreeMap<String, u32>) -> Status {
        match overrides.get(&self.device.key()) {
            Some(&n) => {
                let mut r = self.clone();
                r.now = Some(n);
                r.status()
            }
            None => self.status(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.housekeeping(&ctx);
        self.top_bar(ui);
        self.sidebar(ui);
        self.status_bar(ui);
        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::same(28))).show(
            ui,
            |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
                    Page::Joysticks => self.joysticks_page(ui),
                    Page::Profiles => self.profiles_page(ui),
                    Page::Launch => self.launch_page(ui),
                    Page::Settings => self.settings_page(ui),
                });
            },
        );
        self.dialogs(&ctx);
    }
}

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top")
            .exact_size(56.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(20, 0))
                    .stroke(Stroke::new(1.0, LINE)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("◆").color(ACCENT).size(18.0));
                    ui.label(RichText::new("SC All-In-One").family(bold()).size(17.0));
                    ui.add_space(16.0);
                    let channels = self.all_channels();
                    let several_installs = self.installs.len() > 1;
                    let label = |c: &Channel| {
                        if several_installs {
                            format!(
                                "{} · {}",
                                c.name,
                                c.dir.parent().map(|p| p.display().to_string()).unwrap_or_default()
                            )
                        } else {
                            c.name.clone()
                        }
                    };
                    let selected =
                        self.channel.as_ref().map(label).unwrap_or_else(|| "Star Citizen not found".into());
                    let mut pick = None;
                    egui::ComboBox::from_id_salt("channel")
                        .selected_text(selected)
                        .width(if several_installs { 320.0 } else { 120.0 })
                        .show_ui(ui, |ui| {
                            for c in &channels {
                                let on = self.channel.as_ref() == Some(c);
                                if ui.selectable_label(on, label(c)).clicked() && !on {
                                    pick = Some(c.clone());
                                }
                            }
                        })
                        .response
                        .on_hover_text("Which game version to work on");
                    if let Some(c) = pick {
                        self.select(c);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.game_running() {
                            pill(ui, "●  Star Citizen is running", AMBER);
                        } else {
                            pill(ui, "●  Star Citizen not running", GREEN);
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("nav")
            .exact_size(220.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(14)).stroke(Stroke::new(1.0, LINE)))
            .show(ui, |ui| {
                ui.add_space(6.0);
                let moved = self.moved_count();
                let badge = (moved > 0).then(|| (moved.to_string(), AMBER));
                let items = [
                    (Page::Joysticks, "🎮", "Joysticks", badge),
                    (Page::Profiles, "🗁", "Profiles", None),
                    (Page::Launch, "🚀", "Launch with", None),
                    (Page::Settings, "⚙", "Settings", None),
                ];
                for (page, icon, label, badge) in items {
                    if nav_item(ui, self.page == page, icon, label, badge) {
                        self.page = page;
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.label(weak(format!("v{} · GPL-3.0", env!("CARGO_PKG_VERSION"))).small());
                    if self.data.portable {
                        ui.label(weak("Portable").small()).on_hover_text(format!(
                            "Settings and profiles are kept in {}",
                            self.data.root.display()
                        ));
                    }
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let Some((text, tone, at)) = &self.message else { return };
        if at.elapsed() > Duration::from_secs(12) && *tone != Tone::Bad {
            self.message = None;
            return;
        }
        let (text, tone) = (text.clone(), *tone);
        let color = match tone {
            Tone::Good => GREEN,
            Tone::Bad => RED,
            Tone::Info => ACCENT,
        };
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(color.gamma_multiply(0.12))
                    .inner_margin(Margin::symmetric(20, 10))
                    .stroke(Stroke::new(1.0, color.gamma_multiply(0.4))),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(text).color(Color32::WHITE));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            self.message = None;
                        }
                    });
                });
            });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SC All-In-One")
            .with_app_id("sc-aio")
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("SC All-In-One", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
