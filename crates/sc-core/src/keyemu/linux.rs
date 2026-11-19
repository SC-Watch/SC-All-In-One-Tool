//! Linux: read sticks through evdev, send through uinput.

use super::{LiveStick, Seen, Status, same_device};
use crate::device::DeviceId;
use crate::keymap::{HatDir, Input, KeyProfile};
use crate::keys::Key;
use crate::mapper::{Mapper, Output};
use anyhow::Result;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode, RelativeAxisCode};
use std::collections::{BTreeMap, HashMap};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BTN_JOYSTICK: u16 = 0x120;
const BTN_GAMEPAD_LAST: u16 = 0x13f;
const BTN_TRIGGER_HAPPY: u16 = 0x2c0;
const KEY_MAX: u16 = 0x2ff;
const BTN_MISC: u16 = 0x100;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT3Y: u16 = 0x17;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;

const VIRTUAL_PREFIX: &str = "SC All-In-One";
const TICK: Duration = Duration::from_millis(8);
const RESCAN: Duration = Duration::from_secs(3);

/// The virtual keyboard and mouse, made before the thread starts so a
/// permission problem is reported straight away.
pub struct Ready {
    keyboard: Option<VirtualDevice>,
    mouse: Option<VirtualDevice>,
}

pub fn prepare(send: bool) -> Result<Ready> {
    if !send {
        return Ok(Ready { keyboard: None, mouse: None });
    }
    let mut keys = AttributeSet::<KeyCode>::new();
    for k in Key::all() {
        keys.insert(KeyCode(k.linux_code()));
    }
    let keyboard = VirtualDevice::builder()
        .and_then(|b| b.name("SC All-In-One keyboard").with_keys(&keys))
        .and_then(|b| b.build())
        .map_err(super::NoVirtualDevices)?;

    let mut buttons = AttributeSet::<KeyCode>::new();
    for b in [KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE] {
        buttons.insert(b);
    }
    let mut axes = AttributeSet::<RelativeAxisCode>::new();
    for a in [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y, RelativeAxisCode::REL_WHEEL] {
        axes.insert(a);
    }
    let mouse = VirtualDevice::builder()
        .and_then(|b| b.name("SC All-In-One mouse").with_keys(&buttons))
        .and_then(|b| b.with_relative_axes(&axes))
        .and_then(|b| b.build())
        .map_err(super::NoVirtualDevices)?;
    Ok(Ready { keyboard: Some(keyboard), mouse: Some(mouse) })
}

/// The number the game gives a button: HID buttons 1-16 arrive as
/// BTN_JOYSTICK + n and 17 onwards as BTN_TRIGGER_HAPPY + n.
fn standard_button_number(code: u16) -> Option<u16> {
    match code {
        BTN_JOYSTICK..=0x12f => Some(code - BTN_JOYSTICK + 1),
        BTN_TRIGGER_HAPPY..=KEY_MAX => Some(code - BTN_TRIGGER_HAPPY + 17),
        _ => None,
    }
}

/// Button numbers for every button a device has. Devices using only the
/// joystick ranges get the standard numbers; anything else (gamepads) is
/// numbered in code order.
fn button_numbers(codes: &[u16]) -> HashMap<u16, u16> {
    if codes.iter().all(|c| standard_button_number(*c).is_some()) {
        codes.iter().map(|c| (*c, standard_button_number(*c).expect("checked"))).collect()
    } else {
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.into_iter().enumerate().map(|(i, c)| (c, i as u16 + 1)).collect()
    }
}

struct Stick {
    path: PathBuf,
    dev: Device,
    id: DeviceId,
    buttons: HashMap<u16, u16>,
    hats: usize,
    hat_xy: [(i32, i32); 4],
    profile_stick: Option<usize>,
}

fn device_id(dev: &Device) -> DeviceId {
    let id = dev.input_id();
    let name = dev.name().unwrap_or("Unknown device").trim().to_string();
    DeviceId {
        name,
        guid: Some(format!("{{{:04X}{:04X}-0000-0000-0000-504944564944}}", id.product(), id.vendor())),
    }
}

fn open_sticks(profile: &KeyProfile, known: &[PathBuf]) -> Vec<Stick> {
    let mut out = Vec::new();
    for (path, dev) in evdev::enumerate() {
        if known.contains(&path) || dev.name().is_some_and(|n| n.starts_with(VIRTUAL_PREFIX)) {
            continue;
        }
        let codes: Vec<u16> = dev
            .supported_keys()
            .map(|k| k.iter().map(|c| c.code()).filter(|c| *c >= BTN_MISC).collect())
            .unwrap_or_default();
        let joystick_like =
            codes.iter().any(|c| (BTN_JOYSTICK..=BTN_GAMEPAD_LAST).contains(c) || *c >= BTN_TRIGGER_HAPPY);
        if !joystick_like {
            continue;
        }
        let hats = dev
            .supported_absolute_axes()
            .map(|a| a.iter().filter(|c| (ABS_HAT0X..=ABS_HAT3Y).contains(&c.0)).count() / 2)
            .unwrap_or(0);
        let buttons: Vec<u16> = codes.into_iter().filter(|c| !(0x110..=0x11f).contains(c)).collect();
        if dev.set_nonblocking(true).is_err() {
            continue;
        }
        let id = device_id(&dev);
        let profile_stick =
            profile.sticks.iter().position(|s| s.device.as_ref().is_some_and(|d| same_device(d, &id)));
        out.push(Stick {
            path,
            dev,
            id,
            buttons: button_numbers(&buttons),
            hats,
            hat_xy: [(0, 0); 4],
            profile_stick,
        });
    }
    out
}

fn publish_sticks(status: &Arc<Mutex<Status>>, sticks: &[Stick]) {
    if let Ok(mut s) = status.lock() {
        s.sticks = sticks
            .iter()
            .map(|st| LiveStick {
                device: st.id.clone(),
                buttons: st.buttons.len(),
                hats: st.hats,
                profile_stick: st.profile_stick,
            })
            .collect();
    }
}

pub fn run(profile: KeyProfile, mut ready: Ready, stop: Arc<AtomicBool>, status: Arc<Mutex<Status>>) {
    let start = Instant::now();
    let mut mapper = Mapper::new(&profile);
    let mut sticks = open_sticks(&profile, &[]);
    publish_sticks(&status, &sticks);
    let mut last_scan = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        let now = start.elapsed();
        let mut out: Vec<Output> = Vec::new();
        let mut lost: Vec<usize> = Vec::new();

        for (i, stick) in sticks.iter_mut().enumerate() {
            let events: Vec<InputEvent> = match stick.dev.fetch_events() {
                Ok(evs) => evs.collect(),
                Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                Err(_) => {
                    lost.push(i);
                    continue;
                }
            };
            for ev in events {
                let seen_input = match ev.event_type() {
                    EventType::KEY => {
                        let Some(&number) = stick.buttons.get(&ev.code()) else { continue };
                        if ev.value() == 2 {
                            continue; // auto-repeat
                        }
                        let pressed = ev.value() != 0;
                        if let Some(p) = stick.profile_stick {
                            out.extend(mapper.button(p, number, pressed, now));
                        }
                        pressed.then_some(Input::Button { number })
                    }
                    EventType::ABSOLUTE if (ABS_HAT0X..=ABS_HAT3Y).contains(&ev.code()) => {
                        let offset = (ev.code() - ABS_HAT0X) as usize;
                        let hat = offset / 2;
                        let xy = &mut stick.hat_xy[hat];
                        if offset.is_multiple_of(2) {
                            xy.0 = ev.value()
                        } else {
                            xy.1 = ev.value()
                        }
                        let dir = HatDir::from_xy(xy.0, xy.1);
                        if let Some(p) = stick.profile_stick {
                            out.extend(mapper.hat(p, hat as u8 + 1, dir, now));
                        }
                        dir.map(|direction| Input::Hat { hat: hat as u8 + 1, direction })
                    }
                    _ => None,
                };
                if let Some(input) = seen_input
                    && let Ok(mut s) = status.lock()
                {
                    s.saw(Seen { device: stick.id.clone(), input, at: Instant::now() });
                }
            }
        }

        if !lost.is_empty() {
            for i in lost.into_iter().rev() {
                let gone = sticks.remove(i);
                if let Some(p) = gone.profile_stick {
                    out.extend(mapper.release_stick(p));
                }
            }
            publish_sticks(&status, &sticks);
        }
        if last_scan.elapsed() > RESCAN {
            last_scan = Instant::now();
            let known: Vec<PathBuf> = sticks.iter().map(|s| s.path.clone()).collect();
            let new = open_sticks(&profile, &known);
            if !new.is_empty() {
                sticks.extend(new);
                publish_sticks(&status, &sticks);
            }
        }

        out.extend(mapper.tick(now));
        if let Err(e) = send(&mut ready, &out)
            && let Ok(mut s) = status.lock()
        {
            s.error = Some(format!("Sending input failed: {e}"));
        }
        std::thread::sleep(TICK);
    }

    let _ = send(&mut ready, &mapper.release_all());
    if let Ok(mut s) = status.lock() {
        s.running = false;
    }
}

fn send(ready: &mut Ready, out: &[Output]) -> std::io::Result<()> {
    if out.is_empty() {
        return Ok(());
    }
    let mut keys = Vec::new();
    let mut rel: BTreeMap<u16, i32> = BTreeMap::new();
    for o in out {
        match *o {
            Output::KeyDown(k) => keys.push(InputEvent::new(EventType::KEY.0, k.linux_code(), 1)),
            Output::KeyUp(k) => keys.push(InputEvent::new(EventType::KEY.0, k.linux_code(), 0)),
            Output::Move { dx, dy } => {
                *rel.entry(REL_X).or_default() += dx;
                *rel.entry(REL_Y).or_default() += dy;
            }
            Output::Wheel(n) => *rel.entry(REL_WHEEL).or_default() += n,
        }
    }
    if let Some(kb) = ready.keyboard.as_mut() {
        // One report per key change, so a combination's order is kept.
        for ev in keys {
            kb.emit(&[ev])?;
        }
    }
    let rel: Vec<InputEvent> = rel
        .into_iter()
        .filter(|(_, v)| *v != 0)
        .map(|(c, v)| InputEvent::new(EventType::RELATIVE.0, c, v))
        .collect();
    if let (Some(mouse), false) = (ready.mouse.as_mut(), rel.is_empty()) {
        mouse.emit(&rel)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joystick_buttons_get_the_games_numbers() {
        assert_eq!(standard_button_number(0x120), Some(1)); // BTN_TRIGGER
        assert_eq!(standard_button_number(0x12f), Some(16));
        assert_eq!(standard_button_number(0x2c0), Some(17)); // BTN_TRIGGER_HAPPY1
        assert_eq!(standard_button_number(0x2c4), Some(21));
        assert_eq!(standard_button_number(0x130), None); // gamepad range
        let gamepad = button_numbers(&[0x131, 0x130, 0x133]);
        assert_eq!((gamepad[&0x130], gamepad[&0x131], gamepad[&0x133]), (1, 2, 3));
        let stick = button_numbers(&[0x120, 0x2c4]);
        assert_eq!((stick[&0x120], stick[&0x2c4]), (1, 21));
    }
}
