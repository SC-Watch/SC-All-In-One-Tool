//! Key mapping: joystick buttons and hats that send keys, move the mouse or
//! turn the mouse wheel, like JoyToKey. In Star Citizen this is mostly used
//! to hold a keyboard modifier from a stick button, so stick bindings such as
//! `js3_rctrl+button21` work without touching the keyboard.

use crate::data::DataDir;
use crate::device::DeviceId;
use crate::fsutil::{TextFile, safe_file_name, write_atomic};
use crate::keys::{Key, combo_label};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HatDir {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl HatDir {
    pub const ALL: [HatDir; 8] = [
        HatDir::Up,
        HatDir::UpRight,
        HatDir::Right,
        HatDir::DownRight,
        HatDir::Down,
        HatDir::DownLeft,
        HatDir::Left,
        HatDir::UpLeft,
    ];

    /// JoyToKey numbers directions 1-8 clockwise from up.
    pub fn from_joytokey(n: u8) -> Option<HatDir> {
        HatDir::ALL.get(usize::from(n).checked_sub(1)?).copied()
    }

    /// From axis values -1, 0 or 1 (y is -1 for up). `None` when centred.
    pub fn from_xy(x: i32, y: i32) -> Option<HatDir> {
        Some(match (x.signum(), y.signum()) {
            (0, -1) => HatDir::Up,
            (1, -1) => HatDir::UpRight,
            (1, 0) => HatDir::Right,
            (1, 1) => HatDir::DownRight,
            (0, 1) => HatDir::Down,
            (-1, 1) => HatDir::DownLeft,
            (-1, 0) => HatDir::Left,
            (-1, -1) => HatDir::UpLeft,
            _ => return None,
        })
    }

    /// The two straight directions a diagonal is made of.
    pub fn parts(self) -> Option<(HatDir, HatDir)> {
        match self {
            HatDir::UpRight => Some((HatDir::Up, HatDir::Right)),
            HatDir::DownRight => Some((HatDir::Down, HatDir::Right)),
            HatDir::DownLeft => Some((HatDir::Down, HatDir::Left)),
            HatDir::UpLeft => Some((HatDir::Up, HatDir::Left)),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HatDir::Up => "up",
            HatDir::UpRight => "up-right",
            HatDir::Right => "right",
            HatDir::DownRight => "down-right",
            HatDir::Down => "down",
            HatDir::DownLeft => "down-left",
            HatDir::Left => "left",
            HatDir::UpLeft => "up-left",
        }
    }
}

/// A button (numbered from 1, as the game numbers them) or a hat direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Input {
    Button { number: u16 },
    Hat { hat: u8, direction: HatDir },
}

impl Input {
    pub fn label(&self) -> String {
        match self {
            Input::Button { number } => format!("Button {number}"),
            Input::Hat { hat, direction } => format!("Hat {hat} {}", direction.label()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Action {
    /// Hold these keys while the input is held (up to 4).
    Keys { keys: Vec<Key> },
    /// Move the mouse while held; `dx`/`dy` from -100 to 100, as a share of
    /// the profile's mouse speed. `dy` is negative for up.
    Mouse { dx: i32, dy: i32 },
    /// Turn the mouse wheel, repeating while held: positive scrolls up.
    Wheel { amount: i32 },
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Action::Keys { keys } => combo_label(keys),
            Action::Mouse { dx, dy } => {
                let dir = match (dx.signum(), dy.signum()) {
                    (0, -1) => "up",
                    (0, 1) => "down",
                    (-1, 0) => "left",
                    (1, 0) => "right",
                    (1, -1) => "up-right",
                    (1, 1) => "down-right",
                    (-1, 1) => "down-left",
                    (-1, -1) => "up-left",
                    _ => "nowhere",
                };
                let speed = dx.abs().max(dy.abs());
                if speed == 100 {
                    format!("Move mouse {dir}")
                } else {
                    format!("Move mouse {dir} ({speed}%)")
                }
            }
            Action::Wheel { amount } if *amount > 0 => "Scroll up".into(),
            Action::Wheel { .. } => "Scroll down".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub input: Input,
    pub action: Action,
}

/// One physical stick and what its buttons do.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stick {
    /// Its number in the JoyToKey profile it was imported from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joytokey_number: Option<u8>,
    /// Which device it is, once the user has said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceId>,
    #[serde(default)]
    pub mappings: Vec<Mapping>,
}

impl Stick {
    pub fn title(&self) -> String {
        match (&self.device, self.joytokey_number) {
            (Some(d), _) => d.name.clone(),
            (None, Some(n)) => format!("JoyToKey stick {n}"),
            (None, None) => "Unassigned stick".into(),
        }
    }
}

pub const DEFAULT_MOUSE_SPEED: f32 = 600.0;

fn default_mouse_speed() -> f32 {
    DEFAULT_MOUSE_SPEED
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyProfile {
    pub name: String,
    /// Pixels per second when a mouse mapping is at 100.
    #[serde(default = "default_mouse_speed")]
    pub mouse_speed: f32,
    #[serde(default, rename = "stick")]
    pub sticks: Vec<Stick>,
}

impl KeyProfile {
    pub fn new(name: &str) -> Self {
        KeyProfile { name: name.trim().to_string(), mouse_speed: DEFAULT_MOUSE_SPEED, sticks: Vec::new() }
    }

    pub fn mapping_count(&self) -> usize {
        self.sticks.iter().map(|s| s.mappings.len()).sum()
    }

    pub fn unassigned(&self) -> usize {
        self.sticks.iter().filter(|s| s.device.is_none() && !s.mappings.is_empty()).count()
    }
}

// ---------------------------------------------------------------- storage

fn dir(data: &DataDir) -> PathBuf {
    data.root.join("keymaps")
}

fn path_for(data: &DataDir, name: &str) -> PathBuf {
    dir(data).join(format!("{}.toml", safe_file_name(name)))
}

pub fn list(data: &DataDir) -> Result<Vec<KeyProfile>> {
    let dir = dir(data);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("could not read {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "toml")
            && let Ok(p) = load_file(&path)
        {
            out.push(p);
        }
    }
    out.sort_by_key(|p| p.name.to_lowercase());
    Ok(out)
}

fn load_file(path: &Path) -> Result<KeyProfile> {
    let text = std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("{} could not be read", path.display()))
}

pub fn load(data: &DataDir, name: &str) -> Result<KeyProfile> {
    list(data)?
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
        .with_context(|| format!("there is no key mapping called \"{}\"", name.trim()))
}

pub fn save(data: &DataDir, profile: &KeyProfile) -> Result<()> {
    if profile.name.trim().is_empty() {
        bail!("give the key mapping a name");
    }
    write_atomic(&path_for(data, &profile.name), toml::to_string_pretty(profile)?.as_bytes())
}

pub fn delete(data: &DataDir, name: &str) -> Result<()> {
    let path = path_for(data, name);
    std::fs::remove_file(&path).with_context(|| format!("could not delete {}", path.display()))
}

// ---------------------------------------------------------------- JoyToKey import

#[derive(Clone, Debug)]
pub struct Import {
    pub profile: KeyProfile,
    /// Entries that could not be brought over, with the reason.
    pub skipped: Vec<String>,
}

/// Read a JoyToKey `.cfg` profile.
///
/// Entries look like `Button05=1, A3:52:00:00, 0.000, 0, 0`: type 1 sends up
/// to four keys (Windows virtual-key codes); type 2 is the mouse, with
/// horizontal movement, vertical movement and wheel first
/// (`POV1-1=2, 0,-100, 0, ...` moves the mouse up). Hats are `POVh-d` with
/// directions 1-8 clockwise from up.
pub fn import_joytokey(name: &str, raw: &str) -> Result<Import> {
    let text = TextFile::from_string(raw.to_string()).text;
    let mut profile = KeyProfile::new(name);
    let mut skipped = Vec::new();
    let mut current: Option<u8> = None;
    let mut saw_joystick = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = section.strip_prefix("Joystick ").and_then(|n| n.trim().parse().ok());
            if let Some(n) = current {
                saw_joystick = true;
                profile.sticks.push(Stick { joytokey_number: Some(n), ..Default::default() });
            }
            continue;
        }
        let Some(n) = current else { continue };
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        let where_ = format!("Stick {n} {key}");

        let input = if let Some(num) = key.strip_prefix("Button") {
            match num.parse::<u16>() {
                Ok(number) if number > 0 => Input::Button { number },
                _ => continue,
            }
        } else if let Some(rest) = key.strip_prefix("POV") {
            let parsed = rest.split_once('-').and_then(|(h, d)| {
                let hat: u8 = h.parse().ok()?;
                Some((hat, HatDir::from_joytokey(d.parse().ok()?)?))
            });
            match parsed {
                Some((hat, direction)) => Input::Hat { hat, direction },
                None => continue,
            }
        } else if key.starts_with("Axis") {
            if mapped_type(value) != Some(0) {
                skipped.push(format!("{where_}: axes are not supported yet"));
            }
            continue;
        } else {
            continue; // display names and other settings
        };

        let fields: Vec<&str> = value.split(',').map(str::trim).collect();
        let action = match fields.first().and_then(|t| t.parse::<u32>().ok()) {
            Some(0) | None => continue,
            Some(1) => {
                let mut keys = Vec::new();
                let mut unknown = Vec::new();
                for code in fields.get(1).copied().unwrap_or("").split(':') {
                    match u8::from_str_radix(code.trim(), 16) {
                        Ok(0) | Err(_) => {}
                        Ok(vk) => match Key::from_vk(vk) {
                            Some(k) => keys.push(k),
                            None => unknown.push(format!("{vk:02X}")),
                        },
                    }
                }
                if !unknown.is_empty() {
                    skipped.push(format!("{where_}: unknown key code {}", unknown.join(", ")));
                    continue;
                }
                if keys.is_empty() {
                    continue;
                }
                Action::Keys { keys }
            }
            Some(2) => {
                let num = |i: usize| fields.get(i).and_then(|f| f.parse::<i32>().ok()).unwrap_or(0);
                let (dx, dy, wheel) = (num(1), num(2), num(3));
                if wheel != 0 {
                    Action::Wheel { amount: wheel.signum() }
                } else if dx != 0 || dy != 0 {
                    Action::Mouse { dx: dx.clamp(-100, 100), dy: dy.clamp(-100, 100) }
                } else {
                    skipped.push(format!("{where_}: mouse clicks are not supported yet"));
                    continue;
                }
            }
            Some(t) => {
                skipped.push(format!(
                    "{where_}: JoyToKey action type {t} (such as launching a program) is not supported"
                ));
                continue;
            }
        };
        if let Some(stick) = profile.sticks.last_mut() {
            stick.mappings.push(Mapping { input, action });
        }
    }

    if !saw_joystick {
        bail!("this does not look like a JoyToKey profile: it has no [Joystick] sections");
    }
    profile.sticks.retain(|s| !s.mappings.is_empty());
    Ok(Import { profile, skipped })
}

fn mapped_type(value: &str) -> Option<u32> {
    value.split(',').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_directions() {
        assert_eq!(HatDir::from_joytokey(1), Some(HatDir::Up));
        assert_eq!(HatDir::from_joytokey(3), Some(HatDir::Right));
        assert_eq!(HatDir::from_joytokey(8), Some(HatDir::UpLeft));
        assert_eq!(HatDir::from_joytokey(9), None);
        assert_eq!(HatDir::from_xy(0, -1), Some(HatDir::Up));
        assert_eq!(HatDir::from_xy(1, 1), Some(HatDir::DownRight));
        assert_eq!(HatDir::from_xy(0, 0), None);
    }

    #[test]
    fn profiles_round_trip() {
        let mut p = KeyProfile::new("Test");
        p.sticks.push(Stick {
            joytokey_number: Some(2),
            device: Some(DeviceId::parse(" VKBSim T-Rudder   {011F231D-0000-0000-0000-504944564944}")),
            mappings: vec![
                Mapping {
                    input: Input::Button { number: 5 },
                    action: Action::Keys { keys: crate::keys::parse_combo("RCtrl+R").unwrap() },
                },
                Mapping {
                    input: Input::Hat { hat: 1, direction: HatDir::Up },
                    action: Action::Mouse { dx: 0, dy: -100 },
                },
                Mapping { input: Input::Button { number: 21 }, action: Action::Wheel { amount: 1 } },
            ],
        });
        let text = toml::to_string_pretty(&p).unwrap();
        assert_eq!(toml::from_str::<KeyProfile>(&text).unwrap(), p, "{text}");
    }
}
