//! Remembering which device each joystick number belongs to, noticing when
//! the game has renumbered them, and working out how to put the bindings
//! back on the right devices.
//!
//! The game decides the numbers itself, from the order Windows (or Wine)
//! lists the devices, so the tool cannot choose them. What it can do is move
//! each device's bindings to whatever number the device has now.
//!
//! To know which bindings belong to which device without assuming whether
//! the game rewrote them, it compares content: each remembered device keeps
//! a fingerprint of its bindings (`"spaceship_movement/v_strafe_lateral/x"`
//! and so on), and the js number in the live file whose bindings look most
//! like that fingerprint is where that device's bindings are now.

use crate::actionmaps::{ActionMaps, Renumber};
use crate::device::DeviceId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

/// Below this overlap a number's bindings are not taken to be a device's.
const MIN_SIMILARITY: f32 = 0.4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RememberedDevice {
    pub device: DeviceId,
    pub instance: u32,
    pub fingerprint: BTreeSet<String>,
}

/// The joystick setup as it was when the user said it was right.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JoystickSet {
    pub saved_at: String,
    pub devices: Vec<RememberedDevice>,
}

impl JoystickSet {
    pub fn from_actionmaps(am: &ActionMaps) -> Self {
        let devices = am
            .joysticks
            .iter()
            .map(|j| RememberedDevice {
                device: j.device.clone(),
                instance: j.instance,
                fingerprint: am.fingerprints.get(&j.instance).cloned().unwrap_or_default(),
            })
            .collect();
        JoystickSet { saved_at: crate::now_string(), devices }
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
        let set = toml::from_str(&text)
            .with_context(|| format!("{} is damaged; remember your joysticks again", path.display()))?;
        Ok(Some(set))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        crate::fsutil::write_atomic(path, text.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Its bindings are on the number the game gives it.
    Ok,
    /// The game gives it a different number from where its bindings are.
    Moved,
    /// Remembered, but the game does not list it now.
    NotConnected,
    /// Listed by the game, but not part of the remembered setup.
    New,
    /// Its bindings could not be found, or it cannot be told apart from an
    /// identical device.
    Unclear,
}

/// One device, as the Joysticks page shows it.
#[derive(Clone, Debug)]
pub struct DeviceRow {
    pub device: DeviceId,
    /// Its number when the setup was remembered.
    pub remembered: Option<u32>,
    /// The number its bindings are on now.
    pub bindings_on: Option<u32>,
    /// The number the game gives it now.
    pub now: Option<u32>,
    /// How many bindings it has (not counting cleared defaults).
    pub bindings: usize,
    /// It has bindings to look for.
    pub has_fingerprint: bool,
    /// Another connected device has the same name and hardware ID.
    pub ambiguous: bool,
}

impl DeviceRow {
    pub fn status(&self) -> Status {
        if self.ambiguous {
            return Status::Unclear;
        }
        match (self.remembered, self.now) {
            (None, _) => Status::New,
            (Some(_), None) => Status::NotConnected,
            (Some(_), Some(now)) => match self.bindings_on {
                Some(on) if on == now => Status::Ok,
                Some(_) => Status::Moved,
                None if !self.has_fingerprint => Status::Ok,
                None => Status::Unclear,
            },
        }
    }
}

fn similarity(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    shared as f32 / (a.len() + b.len() - shared) as f32
}

/// Compare the live bindings file with the remembered setup.
pub fn analyse(live: &ActionMaps, set: &JoystickSet) -> Vec<DeviceRow> {
    let dupes = live.duplicate_keys();

    // Find where each remembered device's bindings are: best overlap first.
    let mut pairs: Vec<(f32, usize, u32)> = Vec::new();
    for (i, d) in set.devices.iter().enumerate() {
        for (&n, fp) in &live.fingerprints {
            let s = similarity(&d.fingerprint, fp);
            if s >= MIN_SIMILARITY {
                pairs.push((s, i, n));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut found: BTreeMap<usize, u32> = BTreeMap::new();
    let mut taken: HashSet<u32> = HashSet::new();
    for (_, i, n) in pairs {
        if !found.contains_key(&i) && !taken.contains(&n) {
            found.insert(i, n);
            taken.insert(n);
        }
    }

    let mut rows: Vec<DeviceRow> = set
        .devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let bindings_on = found.get(&i).copied();
            DeviceRow {
                device: d.device.clone(),
                remembered: Some(d.instance),
                bindings_on,
                now: live.instance_of(&d.device),
                bindings: bindings_on.map_or(0, |n| live.bindings_on(n)),
                has_fingerprint: !d.fingerprint.is_empty(),
                ambiguous: dupes.contains(&d.device.key()),
            }
        })
        .collect();

    let remembered: HashSet<String> = set.devices.iter().map(|d| d.device.key()).collect();
    for j in &live.joysticks {
        if !remembered.contains(&j.device.key()) {
            rows.push(DeviceRow {
                device: j.device.clone(),
                remembered: None,
                bindings_on: None,
                now: Some(j.instance),
                bindings: 0,
                has_fingerprint: false,
                ambiguous: dupes.contains(&j.device.key()),
            });
        }
    }
    rows
}

/// Rows for putting a saved file's bindings onto the devices' current
/// numbers. A saved profile is consistent with itself, so each device's
/// bindings are on the number the profile gives it.
pub fn rows_for_restore(saved: &ActionMaps, live: Option<&ActionMaps>) -> Vec<DeviceRow> {
    let dupes = saved.duplicate_keys();
    saved
        .joysticks
        .iter()
        .map(|j| DeviceRow {
            device: j.device.clone(),
            remembered: Some(j.instance),
            bindings_on: Some(j.instance),
            // With no live file to go by, keep the profile's numbers.
            now: match live {
                Some(live) => live.instance_of(&j.device),
                None => Some(j.instance),
            },
            bindings: saved.bindings_on(j.instance),
            has_fingerprint: saved.fingerprints.contains_key(&j.instance),
            ambiguous: dupes.contains(&j.device.key()),
        })
        .collect()
}

/// A worked-out renumbering, with a description of what it does.
#[derive(Clone, Debug, Default)]
pub struct FixPlan {
    pub renumber: Renumber,
    /// Devices whose bindings move: (device, from, to).
    pub moves: Vec<(DeviceId, u32, u32)>,
    /// Bindings moved out of the way to a free number, because the number
    /// they were on now belongs to another device: (label, from, to).
    pub parked: Vec<(String, u32, u32)>,
    /// Devices whose `options` entry changes number (only when a number was
    /// set by hand, or when restoring a profile onto different numbers).
    pub option_changes: usize,
}

impl FixPlan {
    pub fn is_noop(&self) -> bool {
        self.moves.is_empty() && self.parked.is_empty() && self.option_changes == 0
    }
}

/// Work out how to rewrite `doc` (the file whose bindings the rows describe)
/// so that each device's bindings sit on the number in `row.now`.
pub fn plan(rows: &[DeviceRow], doc: &ActionMaps) -> FixPlan {
    let mut fix = FixPlan::default();
    let mut inputs: BTreeMap<u32, u32> = BTreeMap::new();
    let mut pending: Vec<(u32, String, Option<String>)> = Vec::new(); // from, label, device key

    for row in rows.iter().filter(|r| !r.ambiguous) {
        match (row.bindings_on, row.now) {
            (Some(from), Some(to)) if !inputs.contains_key(&from) => {
                inputs.insert(from, to);
                fix.renumber.options.insert(row.device.key(), to);
                if from != to {
                    fix.moves.push((row.device.clone(), from, to));
                }
            }
            (Some(from), None) if !inputs.contains_key(&from) => {
                pending.push((from, row.device.name.clone(), Some(row.device.key())));
            }
            (None, Some(to)) => {
                fix.renumber.options.insert(row.device.key(), to);
            }
            _ => {}
        }
    }

    // Bindings on numbers no device above accounts for.
    let accounted: BTreeSet<u32> = inputs.keys().copied().chain(pending.iter().map(|p| p.0)).collect();
    for &n in doc.fingerprints.keys() {
        if !accounted.contains(&n) {
            let label = match doc.device_at(n) {
                Some(d) => d.name.clone(),
                None => format!("Unknown device (js{n})"),
            };
            pending.push((n, label, None));
        }
    }

    // Leave the rest where they are unless that number is now someone else's.
    let mut used: BTreeSet<u32> = inputs.values().copied().collect();
    used.extend(rows.iter().filter_map(|r| r.now));
    let mut collide = Vec::new();
    for (from, label, key) in pending {
        if used.contains(&from) {
            collide.push((from, label, key));
        } else {
            inputs.insert(from, from);
            used.insert(from);
            if let Some(key) = key {
                fix.renumber.options.insert(key, from);
            }
        }
    }
    for (from, label, key) in collide {
        let free = (1..).find(|n| !used.contains(n)).expect("a free number exists");
        inputs.insert(from, free);
        used.insert(free);
        if let Some(key) = key {
            fix.renumber.options.insert(key, free);
        }
        fix.parked.push((label, from, free));
    }

    fix.renumber.inputs = inputs;
    fix.option_changes = doc
        .joysticks
        .iter()
        .filter(|j| matches!(fix.renumber.options.get(&j.device.key()), Some(&to) if to != j.instance))
        .count();
    fix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(n: &[&str]) -> BTreeSet<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn similarity_is_overlap() {
        assert_eq!(similarity(&set(&["a", "b"]), &set(&["a", "b"])), 1.0);
        assert_eq!(similarity(&set(&["a", "b"]), &set(&["b", "c"])), 1.0 / 3.0);
        assert_eq!(similarity(&set(&[]), &set(&["a"])), 0.0);
    }
}
