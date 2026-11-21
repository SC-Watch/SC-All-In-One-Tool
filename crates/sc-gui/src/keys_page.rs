//! The Key mapping page: JoyToKey-style stick buttons that hold keys or move
//! the mouse.

use crate::picker::{Kind, Purpose};
use crate::theme::*;
use crate::{App, Tone};
use eframe::egui::{self, Align, Id, Layout, RichText, TextEdit, Ui, Vec2};
use sc_core::device::DeviceId;
use sc_core::keyemu::{self, Runner, Seen, same_device};
use sc_core::keymap::{self, Action, HatDir, Input, KeyProfile, Mapping, Stick};
use sc_core::keys;
use std::time::{Duration, Instant};

const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct KeysState {
    pub profiles: Vec<KeyProfile>,
    /// Mapping and sending keys.
    pub runner: Option<Runner>,
    /// Only watching the sticks: to identify one, or to show presses.
    pub watcher: Option<Runner>,
    /// Waiting for a press to tell which device a profile stick is.
    identify: Option<(usize, Instant)>,
    adding: Option<NewMapping>,
    import_report: Option<(String, usize, Vec<String>)>,
    confirm_delete: bool,
    new_name: Option<String>,
    /// Why Start last failed, shown on the page; with a command that fixes it.
    start_error: Option<(String, Option<String>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Keys,
    Mouse,
    Wheel,
}

#[derive(Clone)]
struct NewMapping {
    stick: usize,
    hat: bool,
    number: u16,
    hat_no: u8,
    dir: HatDir,
    kind: ActionKind,
    keys: String,
    mouse: HatDir,
    wheel_up: bool,
    error: Option<String>,
}

impl NewMapping {
    fn new(stick: usize) -> Self {
        NewMapping {
            stick,
            hat: false,
            number: 1,
            hat_no: 1,
            dir: HatDir::Up,
            kind: ActionKind::Keys,
            keys: String::new(),
            mouse: HatDir::Up,
            wheel_up: true,
            error: None,
        }
    }

    fn build(&self) -> Result<Mapping, String> {
        let input = if self.hat {
            Input::Hat { hat: self.hat_no, direction: self.dir }
        } else {
            Input::Button { number: self.number }
        };
        let action = match self.kind {
            ActionKind::Keys => Action::Keys { keys: keys::parse_combo(&self.keys)? },
            ActionKind::Mouse => {
                let (x, y) = dir_vector(self.mouse);
                Action::Mouse { dx: x * 100, dy: y * 100 }
            }
            ActionKind::Wheel => Action::Wheel { amount: if self.wheel_up { 100 } else { -100 } },
        };
        Ok(Mapping { input, action })
    }
}

fn dir_vector(d: HatDir) -> (i32, i32) {
    match d {
        HatDir::Up => (0, -1),
        HatDir::UpRight => (1, -1),
        HatDir::Right => (1, 0),
        HatDir::DownRight => (1, 1),
        HatDir::Down => (0, 1),
        HatDir::DownLeft => (-1, 1),
        HatDir::Left => (-1, 0),
        HatDir::UpLeft => (-1, -1),
    }
}

impl KeysState {
    pub fn load(data: &sc_core::data::DataDir) -> (Self, Option<String>) {
        match keymap::list(data) {
            Ok(profiles) => (KeysState { profiles, ..Default::default() }, None),
            Err(e) => (KeysState::default(), Some(format!("{e:#}"))),
        }
    }

    pub fn is_on(&self) -> bool {
        self.runner.as_ref().is_some_and(|r| r.is_running())
    }

    pub fn busy(&self) -> bool {
        self.runner.is_some() || self.watcher.is_some()
    }

    fn live_status(&self) -> Option<keyemu::Status> {
        self.runner.as_ref().or(self.watcher.as_ref()).map(|r| r.status())
    }
}

/// Things the page asks for, applied after drawing.
enum Do {
    Select(String),
    Import,
    New,
    Delete,
    Start,
    Stop,
    Watch(bool),
    Identify(usize),
    Assign(usize, Option<DeviceId>),
    Remove(usize, usize),
    StartAdding(usize),
    Add,
    CancelAdd,
    MouseSpeed(f32),
    WheelSpeed(f32),
    Autostart(bool),
    Copied,
}

impl App {
    fn keymap_index(&self) -> Option<usize> {
        let profiles = &self.keys.profiles;
        match &self.config.key_mapping {
            Some(name) => profiles.iter().position(|p| p.name.eq_ignore_ascii_case(name)).or(Some(0)),
            None => (!profiles.is_empty()).then_some(0),
        }
        .filter(|i| *i < profiles.len())
    }

    pub fn start_keys(&mut self) {
        let Some(i) = self.keymap_index() else { return };
        let profile = self.keys.profiles[i].clone();
        self.keys.watcher = None;
        self.keys.runner = None;
        match Runner::start(profile, true) {
            Ok(r) => {
                self.keys.runner = Some(r);
                self.keys.start_error = None;
                self.say("Key mapping is on. Keep this app open while you play.", Tone::Good);
            }
            Err(e) => {
                let fix = e
                    .downcast_ref::<keyemu::NoVirtualDevices>()
                    .map(|_| keyemu::UINPUT_FIX_COMMAND.to_string());
                let text = match fix {
                    Some(_) => "Could not create the virtual keyboard and mouse: your user is not allowed to use /dev/uinput yet. Run this in a terminal, then restart the computer:".to_string(),
                    None => format!("{e:#}"),
                };
                self.keys.start_error = Some((text, fix));
                self.say("Key mapping could not start. See the Key mapping page.", Tone::Bad);
            }
        }
    }

    fn restart_if_on(&mut self) {
        if self.keys.runner.is_some() {
            self.keys.runner = None;
            self.start_keys();
        }
    }

    fn save_keymap(&mut self, i: usize) {
        if let Err(e) = keymap::save(&self.data, &self.keys.profiles[i]) {
            self.say(format!("Could not save the key mapping: {e:#}"), Tone::Bad);
        }
        self.restart_if_on();
    }

    fn device_choices(&self, status: Option<&keyemu::Status>) -> Vec<DeviceId> {
        let mut out: Vec<DeviceId> = Vec::new();
        let live =
            status.map(|s| s.sticks.iter().map(|l| l.device.clone()).collect::<Vec<_>>()).unwrap_or_default();
        let game = self
            .joy
            .live
            .as_ref()
            .map(|am| am.joysticks.iter().map(|j| j.device.clone()).collect::<Vec<_>>());
        for d in live.into_iter().chain(game.unwrap_or_default()) {
            if !out.iter().any(|o| same_device(o, &d)) {
                out.push(d);
            }
        }
        out
    }

    pub fn keys_page(&mut self, ui: &mut Ui) {
        ui.heading("Key mapping");
        ui.label(weak(
            "Make stick buttons hold keys or move the mouse, like JoyToKey. Mostly used to hold a modifier such as Right Ctrl from a stick button, for bindings like js3_rctrl+button21.",
        ));
        ui.add_space(14.0);
        let mut todo: Vec<Do> = Vec::new();

        if !keyemu::SUPPORTED {
            card(ui, |ui| {
                ui.label(RichText::new("Key mapping runs on Linux and Windows only.").color(AMBER));
                ui.label(weak("You can import and edit mappings here already."));
            });
            ui.add_space(12.0);
        }

        ui.horizontal(|ui| {
            let current = self.keymap_index();
            if !self.keys.profiles.is_empty() {
                let selected = current.map(|i| self.keys.profiles[i].name.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("keymap").selected_text(&selected).width(260.0).show_ui(
                    ui,
                    |ui| {
                        for p in &self.keys.profiles {
                            if ui.selectable_label(p.name == selected, &p.name).clicked() {
                                todo.push(Do::Select(p.name.clone()));
                            }
                        }
                    },
                );
            }
            if button(ui, "Import JoyToKey profile…").clicked() {
                todo.push(Do::Import);
            }
            if button(ui, "New").clicked() {
                todo.push(Do::New);
            }
            if current.is_some() && button(ui, "Delete").clicked() {
                todo.push(Do::Delete);
            }
        });
        ui.add_space(12.0);

        let Some(i) = self.keymap_index() else {
            card(ui, |ui| {
                section_title(ui, "No key mappings yet");
                ui.label(weak(
                    "Import a JoyToKey profile (.cfg) to start from your existing setup, or start a new one.",
                ));
            });
            self.apply(todo);
            return;
        };
        let profile = self.keys.profiles[i].clone();
        let status = self.keys.live_status();
        let on = self.keys.is_on();

        // On/off.
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon, color, title) = if on {
                    ("●", GREEN, "Key mapping is on")
                } else {
                    ("●", WEAK, "Key mapping is off")
                };
                ui.label(RichText::new(icon).color(color).size(20.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).family(bold()).size(16.0));
                    let sub = match (&status, on) {
                        (Some(s), true) => {
                            let mapped = s.sticks.iter().filter(|l| l.profile_stick.is_some()).count();
                            format!(
                                "{mapped} of {} sticks connected. Keep this app open while you play.",
                                profile.sticks.len()
                            )
                        }
                        _ => format!(
                            "{} mappings on {} sticks.",
                            profile.mapping_count(),
                            profile.sticks.len()
                        ),
                    };
                    ui.label(weak(sub));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if on {
                        if button(ui, "Stop").clicked() {
                            todo.push(Do::Stop);
                        }
                    } else {
                        let start =
                            ui.add_enabled_ui(keyemu::SUPPORTED, |ui| primary_button(ui, "Start")).inner;
                        if start.on_disabled_hover_text("Not available on this system.").clicked() {
                            todo.push(Do::Start);
                        }
                    }
                });
            });
            if let Some(err) = status.as_ref().and_then(|s| s.error.clone()) {
                ui.label(RichText::new(err).color(RED));
            }
            if let (false, Some((text, fix))) = (on, self.keys.start_error.clone()) {
                ui.add_space(4.0);
                ui.label(RichText::new(text).color(RED));
                if let Some(cmd) = fix {
                    ui.horizontal(|ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(12, 16, 22))
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_max_width(ui.available_width() - 140.0);
                                ui.add(egui::Label::new(RichText::new(&cmd).monospace().size(12.0)).wrap());
                            });
                        if ui.button("Copy command").clicked() {
                            ui.ctx().copy_text(cmd.clone());
                            todo.push(Do::Copied);
                        }
                    });
                }
            }
            let unassigned = profile.unassigned();
            if unassigned > 0 {
                ui.label(RichText::new(format!(
                    "{unassigned} stick(s) are not matched to a device yet, so their mappings do nothing. Use Identify below."
                ))
                .color(AMBER));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut auto = self.config.key_mapping_autostart;
                if ui
                    .add_enabled(
                        keyemu::SUPPORTED,
                        egui::Checkbox::new(&mut auto, "Start when the app opens"),
                    )
                    .changed()
                {
                    todo.push(Do::Autostart(auto));
                }
            });
            ui.add_space(4.0);
            egui::Grid::new("speeds").num_columns(2).spacing(Vec2::new(16.0, 6.0)).show(ui, |ui| {
                ui.spacing_mut().slider_width = 220.0;
                ui.label(weak("Mouse speed"));
                let mut speed = profile.mouse_speed;
                let r = ui.add(egui::Slider::new(&mut speed, 100.0..=3000.0).suffix(" px/s").step_by(50.0));
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    todo.push(Do::MouseSpeed(speed));
                } else if r.changed() {
                    self.keys.profiles[i].mouse_speed = speed;
                }
                ui.end_row();
                ui.label(weak("Scroll speed")).on_hover_text(
                    "Wheel clicks per second while a scroll button is held. A tap always scrolls one click.",
                );
                let mut wheel = profile.wheel_speed;
                let r = ui.add(
                    egui::Slider::new(&mut wheel, 2.0..=60.0)
                        .suffix(" clicks/s")
                        .step_by(1.0)
                        .fixed_decimals(0),
                );
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    todo.push(Do::WheelSpeed(wheel));
                } else if r.changed() {
                    self.keys.profiles[i].wheel_speed = wheel;
                }
                ui.end_row();
            });
        });
        ui.add_space(16.0);

        // Sticks.
        let choices = self.device_choices(status.as_ref());
        let recent: Vec<Seen> =
            status.as_ref().map(|s| s.recent.iter().cloned().collect()).unwrap_or_default();
        for (si, stick) in profile.sticks.iter().enumerate() {
            card(ui, |ui| {
                self.stick_card(ui, si, stick, &choices, status.as_ref(), &recent, &mut todo);
            });
            ui.add_space(10.0);
        }
        if profile.sticks.is_empty() {
            ui.label(weak("No sticks in this mapping yet."));
        }
        if button(ui, "Add a stick").clicked() {
            self.keys.profiles[i].sticks.push(Stick::default());
            self.save_keymap(i);
        }

        // What the tool reads.
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "What the tool reads");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !on {
                        let watching = self.keys.watcher.is_some();
                        let label = if watching { "Stop watching" } else { "Watch my sticks" };
                        if ui.add_enabled(keyemu::SUPPORTED, egui::Button::new(label)).clicked() {
                            todo.push(Do::Watch(!watching));
                        }
                    }
                });
            });
            ui.label(weak(
                "Press buttons to see the numbers the tool reads. They should match the game's: binding a button in Star Citizen shows the same number (js3_button21 is button 21).",
            ).small());
            if recent.is_empty() {
                ui.label(weak(if status.is_some() { "Nothing pressed yet." } else { "Not watching." }));
            }
            for seen in recent.iter().take(8) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&seen.device.name).family(bold()));
                    ui.label(RichText::new(seen.input.label()).monospace());
                });
            }
        });

        self.apply(todo);
    }

    #[allow(clippy::too_many_arguments)]
    fn stick_card(
        &mut self,
        ui: &mut Ui,
        si: usize,
        stick: &Stick,
        choices: &[DeviceId],
        status: Option<&keyemu::Status>,
        recent: &[Seen],
        todo: &mut Vec<Do>,
    ) {
        let connected = status.is_some_and(|s| s.sticks.iter().any(|l| l.profile_stick == Some(si)));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(stick.title()).family(bold()).size(16.0));
                if let (Some(n), Some(_)) = (stick.joytokey_number, &stick.device) {
                    ui.label(weak(format!("JoyToKey stick {n}")).small());
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match (&stick.device, status) {
                (None, _) => pill(ui, "Not matched to a device", AMBER),
                (Some(_), Some(_)) if connected => pill(ui, "Connected", GREEN),
                (Some(_), Some(_)) => pill(ui, "Not connected", WEAK),
                (Some(_), None) => {}
            });
        });

        ui.horizontal(|ui| {
            ui.label(weak("Device"));
            let selected = stick.device.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "Choose…".into());
            egui::ComboBox::from_id_salt(("stick-device", si)).selected_text(selected).width(320.0).show_ui(
                ui,
                |ui| {
                    if choices.is_empty() {
                        ui.label(weak("No devices found yet: use Identify"));
                    }
                    for d in choices {
                        let on = stick.device.as_ref().is_some_and(|s| same_device(s, d));
                        if ui.selectable_label(on, format!("{}  ({})", d.name, d.hardware_label())).clicked()
                        {
                            todo.push(Do::Assign(si, Some(d.clone())));
                        }
                    }
                    if stick.device.is_some() && ui.selectable_label(false, "Not matched").clicked() {
                        todo.push(Do::Assign(si, None));
                    }
                },
            );
            match self.keys.identify {
                Some((who, _)) if who == si => {
                    ui.label(RichText::new("Press any button on this stick…").color(ACCENT));
                }
                _ => {
                    let id = ui.add_enabled(keyemu::SUPPORTED, egui::Button::new("Identify…"));
                    if id
                        .on_hover_text("Press a button on the stick to tell the tool which device it is")
                        .clicked()
                    {
                        todo.push(Do::Identify(si));
                    }
                }
            }
        });
        ui.add_space(6.0);

        egui::Grid::new(("maps", si)).num_columns(3).spacing(Vec2::new(28.0, 8.0)).show(ui, |ui| {
            for (mi, m) in stick.mappings.iter().enumerate() {
                ui.label(RichText::new(m.input.label()).monospace());
                ui.label(m.action.label());
                if ui.small_button("✕").on_hover_text("Remove").clicked() {
                    todo.push(Do::Remove(si, mi));
                }
                ui.end_row();
            }
        });
        if stick.mappings.is_empty() {
            ui.label(weak("No mappings yet."));
        }

        let adding_here = self.keys.adding.as_ref().is_some_and(|a| a.stick == si);
        if !adding_here {
            if ui.button("+ Add mapping").clicked() {
                todo.push(Do::StartAdding(si));
            }
            return;
        }
        let last_press = stick
            .device
            .as_ref()
            .and_then(|d| recent.iter().find(|s| same_device(&s.device, d)).map(|s| s.input));
        let Some(form) = self.keys.adding.as_mut() else { return };
        ui.separator();
        egui::Grid::new(("add", si)).num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
            ui.label(weak("When"));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("add-input", si))
                    .selected_text(if form.hat { "Hat" } else { "Button" })
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.hat, false, "Button");
                        ui.selectable_value(&mut form.hat, true, "Hat");
                    });
                if form.hat {
                    ui.add(egui::DragValue::new(&mut form.hat_no).range(1..=4).prefix("hat "));
                    egui::ComboBox::from_id_salt(("add-dir", si)).selected_text(form.dir.label()).show_ui(
                        ui,
                        |ui| {
                            for d in HatDir::ALL {
                                ui.selectable_value(&mut form.dir, d, d.label());
                            }
                        },
                    );
                } else {
                    ui.add(egui::DragValue::new(&mut form.number).range(1..=128));
                }
                if let Some(input) = last_press
                    && ui.button("Use last press").on_hover_text(input.label()).clicked()
                {
                    match input {
                        Input::Button { number } => {
                            form.hat = false;
                            form.number = number;
                        }
                        Input::Hat { hat, direction } => {
                            form.hat = true;
                            form.hat_no = hat;
                            form.dir = direction;
                        }
                    }
                }
            });
            ui.end_row();
            ui.label(weak("Do"));
            ui.horizontal(|ui| {
                let label = match form.kind {
                    ActionKind::Keys => "Hold keys",
                    ActionKind::Mouse => "Move mouse",
                    ActionKind::Wheel => "Scroll",
                };
                egui::ComboBox::from_id_salt(("add-kind", si)).selected_text(label).width(120.0).show_ui(
                    ui,
                    |ui| {
                        ui.selectable_value(&mut form.kind, ActionKind::Keys, "Hold keys");
                        ui.selectable_value(&mut form.kind, ActionKind::Mouse, "Move mouse");
                        ui.selectable_value(&mut form.kind, ActionKind::Wheel, "Scroll");
                    },
                );
                match form.kind {
                    ActionKind::Keys => {
                        ui.add(
                            TextEdit::singleline(&mut form.keys)
                                .hint_text("e.g. RCtrl or RCtrl+R")
                                .desired_width(200.0),
                        );
                    }
                    ActionKind::Mouse => {
                        egui::ComboBox::from_id_salt(("add-mouse", si))
                            .selected_text(form.mouse.label())
                            .show_ui(ui, |ui| {
                                for d in HatDir::ALL {
                                    ui.selectable_value(&mut form.mouse, d, d.label());
                                }
                            });
                    }
                    ActionKind::Wheel => {
                        egui::ComboBox::from_id_salt(("add-wheel", si))
                            .selected_text(if form.wheel_up { "up" } else { "down" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut form.wheel_up, true, "up");
                                ui.selectable_value(&mut form.wheel_up, false, "down");
                            });
                    }
                }
            });
            ui.end_row();
        });
        if let Some(e) = &form.error {
            ui.label(RichText::new(e).color(RED));
        }
        ui.horizontal(|ui| {
            if primary_button(ui, "Add").clicked() {
                todo.push(Do::Add);
            }
            if button(ui, "Cancel").clicked() {
                todo.push(Do::CancelAdd);
            }
        });
    }

    fn apply(&mut self, todo: Vec<Do>) {
        let current = self.keymap_index();
        for action in todo {
            match action {
                Do::Select(name) => {
                    self.config.key_mapping = Some(name);
                    self.save_config();
                    self.keys.adding = None;
                    self.restart_if_on();
                }
                Do::Import => self.import_joytokey(),
                Do::New => self.keys.new_name = Some(String::new()),
                Do::Delete => self.keys.confirm_delete = true,
                Do::Start => self.start_keys(),
                Do::Stop => {
                    self.keys.runner = None;
                    self.say("Key mapping is off.", Tone::Info);
                }
                Do::Watch(on) => {
                    self.keys.watcher = None;
                    if on {
                        match Runner::start(KeyProfile::new("watch"), false) {
                            Ok(r) => self.keys.watcher = Some(r),
                            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
                        }
                    }
                }
                Do::Identify(si) => {
                    if !self.keys.busy() {
                        match Runner::start(KeyProfile::new("watch"), false) {
                            Ok(r) => self.keys.watcher = Some(r),
                            Err(e) => {
                                self.say(format!("{e:#}"), Tone::Bad);
                                continue;
                            }
                        }
                    }
                    self.keys.identify = Some((si, Instant::now()));
                }
                Do::Assign(si, device) => {
                    if let Some(i) = current {
                        self.assign(i, si, device);
                    }
                }
                Do::Remove(si, mi) => {
                    if let Some(i) = current {
                        self.keys.profiles[i].sticks[si].mappings.remove(mi);
                        self.save_keymap(i);
                    }
                }
                Do::StartAdding(si) => self.keys.adding = Some(NewMapping::new(si)),
                Do::CancelAdd => self.keys.adding = None,
                Do::Add => {
                    let (Some(i), Some(form)) = (current, self.keys.adding.clone()) else { continue };
                    match form.build() {
                        Ok(m) => {
                            let maps = &mut self.keys.profiles[i].sticks[form.stick].mappings;
                            maps.retain(|x| x.input != m.input);
                            maps.push(m);
                            self.keys.adding = None;
                            self.save_keymap(i);
                        }
                        Err(e) => {
                            if let Some(f) = self.keys.adding.as_mut() {
                                f.error = Some(e);
                            }
                        }
                    }
                }
                Do::WheelSpeed(speed) => {
                    if let Some(i) = current {
                        self.keys.profiles[i].wheel_speed = speed;
                        self.save_keymap(i);
                    }
                }
                Do::MouseSpeed(speed) => {
                    if let Some(i) = current {
                        self.keys.profiles[i].mouse_speed = speed;
                        self.save_keymap(i);
                    }
                }
                Do::Copied => self.say("Copied. Paste it into a terminal.", Tone::Info),
                Do::Autostart(on) => {
                    self.config.key_mapping_autostart = on;
                    self.save_config();
                }
            }
        }
    }

    fn assign(&mut self, i: usize, si: usize, device: Option<DeviceId>) {
        if let Some(d) = &device {
            // One device per stick: take it off any other stick first.
            for (other, s) in self.keys.profiles[i].sticks.iter_mut().enumerate() {
                if other != si && s.device.as_ref().is_some_and(|x| same_device(x, d)) {
                    s.device = None;
                }
            }
        }
        self.keys.profiles[i].sticks[si].device = device;
        self.save_keymap(i);
    }

    fn import_joytokey(&mut self) {
        self.pick(
            Purpose::ImportJoyToKey,
            "Choose a JoyToKey profile",
            Kind::File(Some(("JoyToKey profile", &["cfg"]))),
        );
    }

    pub fn import_joytokey_file(&mut self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.say(format!("Could not read {}: {e}", path.display()), Tone::Bad);
                return;
            }
        };
        let base = path
            .file_stem()
            .map(|s| s.to_string_lossy().replace('_', " "))
            .unwrap_or_else(|| "Imported".into());
        let mut name = base.clone();
        let mut n = 2;
        while self.keys.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
            name = format!("{base} {n}");
            n += 1;
        }
        match keymap::import_joytokey(&name, &text) {
            Ok(import) => {
                if let Err(e) = keymap::save(&self.data, &import.profile) {
                    self.say(format!("{e:#}"), Tone::Bad);
                    return;
                }
                let count = import.profile.mapping_count();
                self.keys.profiles.push(import.profile);
                self.keys.profiles.sort_by_key(|p| p.name.to_lowercase());
                self.config.key_mapping = Some(name.clone());
                self.save_config();
                self.keys.import_report = Some((name, count, import.skipped));
            }
            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
        }
    }

    /// Each frame: finish identifying a stick when a press arrives.
    pub fn keys_housekeeping(&mut self, ctx: &egui::Context) {
        if self.keys.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(r) = &self.keys.runner
            && !r.is_running()
        {
            let err = r.status().error;
            self.keys.runner = None;
            self.say(err.unwrap_or_else(|| "Key mapping stopped.".into()), Tone::Bad);
        }
        let Some((si, since)) = self.keys.identify else { return };
        if since.elapsed() > IDENTIFY_TIMEOUT {
            self.keys.identify = None;
            if self.keys.runner.is_some() || self.keys.watcher.is_some() {
                self.say(
                    "No press was seen. Try Identify again, or pick the device from the list.",
                    Tone::Info,
                );
            }
            return;
        }
        let pressed = self.keys.live_status().and_then(|s| s.recent.iter().find(|x| x.at > since).cloned());
        if let (Some(seen), Some(i)) = (pressed, self.keymap_index()) {
            self.keys.identify = None;
            self.assign(i, si, Some(seen.device.clone()));
            self.say(format!("That stick is {}.", seen.device.name), Tone::Good);
        }
    }

    pub fn keys_dialogs(&mut self, ctx: &egui::Context) {
        if let Some((name, count, skipped)) = self.keys.import_report.clone() {
            let resp = egui::Modal::new(Id::unique("import-report")).show(ctx, |ui| {
                ui.set_width(520.0);
                section_title(ui, &format!("Imported \"{name}\""));
                ui.label(format!("{count} mappings."));
                if !skipped.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Not imported:").family(bold()));
                    for s in &skipped {
                        ui.label(weak(s).small());
                    }
                }
                ui.add_space(6.0);
                ui.label(weak(
                    "JoyToKey numbers sticks in its own order, so tell the tool which device each one is: use Identify on each stick and press a button on it.",
                ));
                ui.add_space(10.0);
                primary_button(ui, "OK").clicked()
            });
            if resp.inner || resp.should_close() {
                self.keys.import_report = None;
            }
        }

        if let Some(mut name) = self.keys.new_name.clone() {
            let mut done = false;
            let mut create = false;
            let resp = egui::Modal::new(Id::unique("new-keymap")).show(ctx, |ui| {
                section_title(ui, "New key mapping");
                ui.add(TextEdit::singleline(&mut name).hint_text("Name").desired_width(280.0));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, "Create").clicked() && !name.trim().is_empty() {
                        create = true;
                    }
                    if button(ui, "Cancel").clicked() {
                        done = true;
                    }
                });
            });
            if create {
                if self.keys.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(name.trim())) {
                    self.say("A key mapping with that name exists.", Tone::Bad);
                } else {
                    let mut p = KeyProfile::new(&name);
                    p.sticks.push(Stick::default());
                    match keymap::save(&self.data, &p) {
                        Ok(()) => {
                            self.config.key_mapping = Some(p.name.clone());
                            self.save_config();
                            self.keys.profiles.push(p);
                            self.keys.profiles.sort_by_key(|p| p.name.to_lowercase());
                            self.restart_if_on();
                        }
                        Err(e) => self.say(format!("{e:#}"), Tone::Bad),
                    }
                    done = true;
                }
            }
            self.keys.new_name = if done || resp.should_close() { None } else { Some(name) };
        }

        if self.keys.confirm_delete {
            let Some(i) = self.keymap_index() else {
                self.keys.confirm_delete = false;
                return;
            };
            let name = self.keys.profiles[i].name.clone();
            let mut done = false;
            let resp = egui::Modal::new(Id::unique("delete-keymap")).show(ctx, |ui| {
                section_title(ui, &format!("Delete \"{name}\"?"));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let del = egui::Button::new(RichText::new("Delete").color(ON_ACCENT).family(bold()))
                        .fill(RED)
                        .min_size(Vec2::new(0.0, 36.0));
                    if ui.add(del).clicked() {
                        match keymap::delete(&self.data, &name) {
                            Ok(()) => {
                                self.keys.profiles.remove(i);
                                self.keys.runner = None;
                                self.config.key_mapping = self.keys.profiles.first().map(|p| p.name.clone());
                                self.save_config();
                            }
                            Err(e) => self.say(format!("{e:#}"), Tone::Bad),
                        }
                        done = true;
                    }
                    if button(ui, "Cancel").clicked() {
                        done = true;
                    }
                });
            });
            if done || resp.should_close() {
                self.keys.confirm_delete = false;
            }
        }
    }
}
