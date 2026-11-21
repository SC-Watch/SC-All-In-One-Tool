//! Windows: read sticks through Raw Input, send keys and mouse through
//! SendInput.
//!
//! Raw Input is delivered to a hidden message-only window even while the game
//! has the focus, reports every button a stick has (DirectInput-style
//! numbering: HID button usage 1 is button 1) and its hats, and gives each
//! device's USB IDs for matching. Keys are sent as scan codes, which games
//! read more reliably than virtual-key codes.

use super::{LiveStick, Seen, Status, same_device};
use crate::device::DeviceId;
use crate::keymap::{HatDir, Input, KeyProfile};
use crate::keys::Key;
use crate::mapper::{Mapper, Output};
use std::collections::{BTreeSet, HashMap};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, HidD_GetProductString,
    HidP_GetButtonCaps, HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input,
    HidP_MaxUsageListLength, PHIDP_PREPARSED_DATA,
};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC_EX, MOUSEEVENTF_MOVE, MOUSEEVENTF_WHEEL, MOUSEINPUT, MapVirtualKeyW,
    SendInput,
};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTDEVICELIST, RAWINPUTHEADER, RID_DEVICE_INFO, RID_DEVICE_INFO_HID, RID_INPUT, RIDEV_DEVNOTIFY,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIDI_DEVICEINFO, RIDI_DEVICENAME, RIDI_PREPARSEDDATA, RIM_TYPEHID,
    RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, MSG, MsgWaitForMultipleObjects,
    PM_REMOVE, PeekMessageW, QS_ALLINPUT, WM_INPUT, WM_INPUT_DEVICE_CHANGE,
};

const TICK_MS: u32 = 8;
const PAGE_GENERIC: u16 = 0x01;
/// Joystick, gamepad and multi-axis controller.
const STICK_USAGES: [u16; 3] = [0x04, 0x05, 0x08];
const PAGE_BUTTON: u16 = 0x09;
const USAGE_HAT: u16 = 0x39;
const GIDC_ARRIVAL: usize = 1;
const GIDC_REMOVAL: usize = 2;

pub struct Ready {
    send: bool,
}

pub fn prepare(send: bool) -> anyhow::Result<Ready> {
    Ok(Ready { send })
}

struct Hat {
    link: u16,
    min: i32,
    max: i32,
}

struct Dev {
    id: DeviceId,
    /// HID preparsed data, kept 8-byte aligned.
    preparsed: Vec<u64>,
    max_buttons: u32,
    /// Which buttons each input report carries, by report ID (0 when the
    /// device does not number its reports).
    report_buttons: HashMap<u8, BTreeSet<u16>>,
    numbered_reports: bool,
    hats: Vec<Hat>,
    pressed: BTreeSet<u16>,
    hat_dirs: Vec<Option<HatDir>>,
    profile_stick: Option<usize>,
}

impl Dev {
    fn pp(&self) -> PHIDP_PREPARSED_DATA {
        self.preparsed.as_ptr() as PHIDP_PREPARSED_DATA
    }
}

/// HID hat value to a direction: 8 positions clockwise from up (or 4), and
/// anything outside the range is centred.
fn hat_dir(value: i32, min: i32, max: i32) -> Option<HatDir> {
    if value < min || value > max {
        return None;
    }
    let step = match max - min + 1 {
        8 => 1,
        4 => 2,
        _ => return None,
    };
    HatDir::ALL.get(((value - min) * step) as usize).copied()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn device_info(h: HANDLE) -> Option<RID_DEVICE_INFO_HID> {
    let mut info: RID_DEVICE_INFO = unsafe { zeroed() };
    info.cbSize = size_of::<RID_DEVICE_INFO>() as u32;
    let mut size = info.cbSize;
    let r =
        unsafe { GetRawInputDeviceInfoW(h, RIDI_DEVICEINFO, &mut info as *mut _ as *mut c_void, &mut size) };
    if r == u32::MAX || r == 0 || info.dwType != RIM_TYPEHID {
        return None;
    }
    Some(unsafe { info.Anonymous.hid })
}

unsafe fn device_path(h: HANDLE) -> Option<Vec<u16>> {
    let mut chars = 0u32;
    unsafe { GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, null_mut(), &mut chars) };
    if chars == 0 {
        return None;
    }
    let mut buf = vec![0u16; chars as usize + 1];
    let r =
        unsafe { GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, buf.as_mut_ptr() as *mut c_void, &mut chars) };
    (r != u32::MAX && r != 0).then_some(buf)
}

/// The name the device reports, e.g. "VKBsim Gladiator EVO R".
unsafe fn product_name(path: &[u16]) -> Option<String> {
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut buf = [0u16; 128];
    let ok =
        unsafe { HidD_GetProductString(handle, buf.as_mut_ptr() as *mut c_void, (buf.len() * 2) as u32) };
    unsafe { CloseHandle(handle) };
    if !ok {
        return None;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let name = String::from_utf16_lossy(&buf[..end]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

unsafe fn preparsed_data(h: HANDLE) -> Option<Vec<u64>> {
    let mut bytes = 0u32;
    unsafe { GetRawInputDeviceInfoW(h, RIDI_PREPARSEDDATA, null_mut(), &mut bytes) };
    if bytes == 0 {
        return None;
    }
    let mut buf = vec![0u64; (bytes as usize).div_ceil(8)];
    let r =
        unsafe { GetRawInputDeviceInfoW(h, RIDI_PREPARSEDDATA, buf.as_mut_ptr() as *mut c_void, &mut bytes) };
    (r != u32::MAX && r != 0).then_some(buf)
}

/// Open a raw input device if it is a stick (or gamepad); `None` otherwise.
unsafe fn open_device(h: HANDLE, profile: &KeyProfile) -> Option<Dev> {
    let info = unsafe { device_info(h) }?;
    if info.usUsagePage != PAGE_GENERIC || !STICK_USAGES.contains(&info.usUsage) {
        return None;
    }
    let (vid, pid) = (info.dwVendorId as u16, info.dwProductId as u16);
    let name = unsafe { device_path(h) }
        .and_then(|p| unsafe { product_name(&p) })
        .unwrap_or_else(|| format!("Device {vid:04X}:{pid:04X}"));
    let id = DeviceId { name, guid: Some(format!("{{{pid:04X}{vid:04X}-0000-0000-0000-504944564944}}")) };

    let preparsed = unsafe { preparsed_data(h) }?;
    let pp = preparsed.as_ptr() as PHIDP_PREPARSED_DATA;
    let mut caps: HIDP_CAPS = unsafe { zeroed() };
    if unsafe { HidP_GetCaps(pp, &mut caps) } != HIDP_STATUS_SUCCESS {
        return None;
    }
    let max_buttons = unsafe { HidP_MaxUsageListLength(HidP_Input, PAGE_BUTTON, pp) };

    let mut count = caps.NumberInputButtonCaps;
    let mut buttons: Vec<HIDP_BUTTON_CAPS> = vec![unsafe { zeroed() }; count as usize];
    let mut report_buttons: HashMap<u8, BTreeSet<u16>> = HashMap::new();
    if count > 0
        && unsafe { HidP_GetButtonCaps(HidP_Input, buttons.as_mut_ptr(), &mut count, pp) }
            == HIDP_STATUS_SUCCESS
    {
        for b in buttons[..count as usize].iter().filter(|b| b.UsagePage == PAGE_BUTTON) {
            let (lo, hi) = unsafe {
                if b.IsRange {
                    (b.Anonymous.Range.UsageMin, b.Anonymous.Range.UsageMax)
                } else {
                    (b.Anonymous.NotRange.Usage, b.Anonymous.NotRange.Usage)
                }
            };
            report_buttons.entry(b.ReportID).or_default().extend(lo..=hi);
        }
    }
    let numbered_reports = report_buttons.keys().any(|id| *id != 0);

    let mut count = caps.NumberInputValueCaps;
    let mut values: Vec<HIDP_VALUE_CAPS> = vec![unsafe { zeroed() }; count as usize];
    let mut hats = Vec::new();
    if count > 0
        && unsafe { HidP_GetValueCaps(HidP_Input, values.as_mut_ptr(), &mut count, pp) }
            == HIDP_STATUS_SUCCESS
    {
        for v in &values[..count as usize] {
            let is_hat = v.UsagePage == PAGE_GENERIC
                && unsafe {
                    if v.IsRange {
                        (v.Anonymous.Range.UsageMin..=v.Anonymous.Range.UsageMax).contains(&USAGE_HAT)
                    } else {
                        v.Anonymous.NotRange.Usage == USAGE_HAT
                    }
                };
            if is_hat {
                hats.push(Hat { link: v.LinkCollection, min: v.LogicalMin, max: v.LogicalMax });
            }
        }
    }
    let profile_stick =
        profile.sticks.iter().position(|s| s.device.as_ref().is_some_and(|d| same_device(d, &id)));
    Some(Dev {
        id,
        preparsed,
        max_buttons,
        report_buttons,
        numbered_reports,
        hat_dirs: vec![None; hats.len()],
        hats,
        pressed: BTreeSet::new(),
        profile_stick,
    })
}

/// Every stick connected now, keyed by raw input handle.
unsafe fn enumerate(profile: &KeyProfile) -> HashMap<isize, Option<Dev>> {
    let mut out = HashMap::new();
    let mut n = 0u32;
    let size = size_of::<RAWINPUTDEVICELIST>() as u32;
    unsafe { GetRawInputDeviceList(null_mut(), &mut n, size) };
    let mut list: Vec<RAWINPUTDEVICELIST> = vec![unsafe { zeroed() }; n as usize];
    let got = unsafe { GetRawInputDeviceList(list.as_mut_ptr(), &mut n, size) };
    if got == u32::MAX {
        return out;
    }
    for d in &list[..got as usize] {
        if d.dwType == RIM_TYPEHID {
            out.insert(d.hDevice as isize, unsafe { open_device(d.hDevice, profile) });
        }
    }
    out
}

fn publish(status: &Arc<Mutex<Status>>, devs: &HashMap<isize, Option<Dev>>) {
    if let Ok(mut s) = status.lock() {
        s.sticks = devs
            .values()
            .flatten()
            .map(|d| LiveStick {
                device: d.id.clone(),
                buttons: d.max_buttons as usize,
                hats: d.hats.len(),
                profile_stick: d.profile_stick,
            })
            .collect();
        s.sticks.sort_by(|a, b| a.device.name.cmp(&b.device.name));
    }
}

unsafe fn register(hwnd: HWND, add: bool) -> bool {
    let flags = if add { RIDEV_INPUTSINK | RIDEV_DEVNOTIFY } else { RIDEV_REMOVE };
    let target = if add { hwnd } else { null_mut() };
    let list: Vec<RAWINPUTDEVICE> = STICK_USAGES
        .iter()
        .map(|u| RAWINPUTDEVICE {
            usUsagePage: PAGE_GENERIC,
            usUsage: *u,
            dwFlags: flags,
            hwndTarget: target,
        })
        .collect();
    unsafe {
        RegisterRawInputDevices(list.as_ptr(), list.len() as u32, size_of::<RAWINPUTDEVICE>() as u32) != 0
    }
}

/// The HID reports in one WM_INPUT, if it came from a HID device.
unsafe fn read_reports(lparam: isize) -> Option<(isize, Vec<Vec<u8>>)> {
    let hraw = lparam as HRAWINPUT;
    let header = size_of::<RAWINPUTHEADER>() as u32;
    let mut size = 0u32;
    unsafe { GetRawInputData(hraw, RID_INPUT, null_mut(), &mut size, header) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let got = unsafe { GetRawInputData(hraw, RID_INPUT, buf.as_mut_ptr() as *mut c_void, &mut size, header) };
    if got == u32::MAX || got == 0 {
        return None;
    }
    // The reports run past the end of RAWHID's one-byte `bRawData`, so work
    // from raw pointers into the whole buffer rather than references.
    let raw = buf.as_ptr() as *const RAWINPUT;
    let (kind, device) = unsafe { ((*raw).header.dwType, (*raw).header.hDevice) };
    if kind != RIM_TYPEHID {
        return None;
    }
    let hid = unsafe { std::ptr::addr_of!((*raw).data.hid) };
    let (each, count) = unsafe { ((*hid).dwSizeHid as usize, (*hid).dwCount as usize) };
    let base = unsafe { std::ptr::addr_of!((*hid).bRawData) } as *const u8;
    let end = buf.as_ptr() as usize + size as usize;
    if base as usize + each * count > end {
        return None;
    }
    let reports = (0..count)
        .map(|k| unsafe { std::slice::from_raw_parts(base.add(k * each), each) }.to_vec())
        .collect();
    Some((device as isize, reports))
}

enum Change {
    Button(u16, bool),
    Hat(u8, Option<HatDir>),
}

/// What changed on a device in one report.
unsafe fn changes(dev: &mut Dev, report: &mut [u8]) -> Vec<Change> {
    let mut out = Vec::new();
    let mut usages = vec![0u16; dev.max_buttons.max(1) as usize];
    let mut len = usages.len() as u32;
    let ok = unsafe {
        HidP_GetUsages(
            HidP_Input,
            PAGE_BUTTON,
            0,
            usages.as_mut_ptr(),
            &mut len,
            dev.pp(),
            report.as_mut_ptr(),
            report.len() as u32,
        )
    } == HIDP_STATUS_SUCCESS;
    // Only the buttons this report carries can change; the rest keep their
    // state (otherwise a stick that splits its buttons over several reports
    // would seem to let go of held buttons between them).
    let report_id = if dev.numbered_reports { report.first().copied().unwrap_or(0) } else { 0 };
    if ok && let Some(in_report) = dev.report_buttons.get(&report_id) {
        let now: BTreeSet<u16> = usages[..len as usize].iter().copied().collect();
        for (button, pressed) in super::report_button_changes(&mut dev.pressed, in_report, &now) {
            out.push(Change::Button(button, pressed));
        }
    }
    for (i, hat) in dev.hats.iter().enumerate() {
        let mut value = 0u32;
        let ok = unsafe {
            HidP_GetUsageValue(
                HidP_Input,
                PAGE_GENERIC,
                hat.link,
                USAGE_HAT,
                &mut value,
                dev.pp(),
                report.as_ptr(),
                report.len() as u32,
            )
        } == HIDP_STATUS_SUCCESS;
        if ok {
            let dir = hat_dir(value as i32, hat.min, hat.max);
            if dev.hat_dirs[i] != dir {
                dev.hat_dirs[i] = dir;
                out.push(Change::Hat(i as u8 + 1, dir));
            }
        }
    }
    out
}

pub fn run(profile: KeyProfile, ready: Ready, stop: Arc<AtomicBool>, status: Arc<Mutex<Status>>) {
    let fail = |msg: &str| {
        if let Ok(mut s) = status.lock() {
            s.error = Some(msg.to_string());
            s.running = false;
        }
    };
    // A message-only window of the built-in STATIC class receives the input.
    let class = wide("STATIC");
    let title = wide("SC All-In-One key mapping");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        fail("Could not create the window that receives stick input.");
        return;
    }
    if !unsafe { register(hwnd, true) } {
        fail("Windows did not allow reading the sticks (raw input registration failed).");
        unsafe { DestroyWindow(hwnd) };
        return;
    }

    let mut devs = unsafe { enumerate(&profile) };
    publish(&status, &devs);
    let start = Instant::now();
    let mut mapper = Mapper::new(&profile);
    let mut msg: MSG = unsafe { zeroed() };

    while !stop.load(Ordering::Relaxed) {
        unsafe { MsgWaitForMultipleObjects(0, null(), 0, TICK_MS, QS_ALLINPUT) };
        let now = start.elapsed();
        let mut out: Vec<Output> = Vec::new();
        while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            match msg.message {
                WM_INPUT => {
                    if let Some((handle, reports)) = unsafe { read_reports(msg.lParam) } {
                        let dev = devs
                            .entry(handle)
                            .or_insert_with(|| unsafe { open_device(handle as HANDLE, &profile) });
                        if let Some(dev) = dev.as_mut() {
                            for mut report in reports {
                                for change in unsafe { changes(dev, &mut report) } {
                                    let (input, outs) = match change {
                                        Change::Button(number, pressed) => {
                                            let outs = dev
                                                .profile_stick
                                                .map(|p| mapper.button(p, number, pressed, now))
                                                .unwrap_or_default();
                                            (pressed.then_some(Input::Button { number }), outs)
                                        }
                                        Change::Hat(hat, dir) => {
                                            let outs = dev
                                                .profile_stick
                                                .map(|p| mapper.hat(p, hat, dir, now))
                                                .unwrap_or_default();
                                            (dir.map(|direction| Input::Hat { hat, direction }), outs)
                                        }
                                    };
                                    out.extend(outs);
                                    if let Some(input) = input
                                        && let Ok(mut s) = status.lock()
                                    {
                                        s.saw(Seen { device: dev.id.clone(), input, at: Instant::now() });
                                    }
                                }
                            }
                        }
                    }
                }
                WM_INPUT_DEVICE_CHANGE => {
                    let handle = msg.lParam;
                    match msg.wParam {
                        GIDC_ARRIVAL => {
                            devs.insert(handle, unsafe { open_device(handle as HANDLE, &profile) });
                        }
                        GIDC_REMOVAL => {
                            if let Some(Some(gone)) = devs.remove(&handle)
                                && let Some(p) = gone.profile_stick
                            {
                                out.extend(mapper.release_stick(p));
                            }
                        }
                        _ => {}
                    }
                    publish(&status, &devs);
                }
                _ => {}
            }
            // Lets Windows clean up after WM_INPUT.
            unsafe { DispatchMessageW(&msg) };
        }
        out.extend(mapper.tick(now));
        if ready.send {
            unsafe { send(&out) };
        }
    }

    if ready.send {
        unsafe { send(&mapper.release_all()) };
    }
    unsafe {
        register(hwnd, false);
        DestroyWindow(hwnd);
    }
    if let Ok(mut s) = status.lock() {
        s.running = false;
    }
}

fn key_input(key: Key, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(u32::from(key.vk()), MAPVK_VK_TO_VSC_EX) };
    let ki = if scan == 0 {
        // No scan code known: fall back to the virtual-key code.
        KEYBDINPUT {
            wVk: u16::from(key.vk()),
            wScan: 0,
            dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
            time: 0,
            dwExtraInfo: 0,
        }
    } else {
        let mut flags = KEYEVENTF_SCANCODE;
        if scan & 0xFF00 == 0xE000 || scan & 0xFF00 == 0xE100 {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        KEYBDINPUT { wVk: 0, wScan: (scan & 0xFF) as u16, dwFlags: flags, time: 0, dwExtraInfo: 0 }
    };
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } }
}

fn mouse_input(flags: u32, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

unsafe fn send(out: &[Output]) {
    if out.is_empty() {
        return;
    }
    let mut inputs: Vec<INPUT> = Vec::new();
    let (mut mx, mut my, mut wheel) = (0, 0, 0);
    for o in out {
        match *o {
            Output::KeyDown(k) => inputs.push(key_input(k, false)),
            Output::KeyUp(k) => inputs.push(key_input(k, true)),
            Output::Move { dx, dy } => {
                mx += dx;
                my += dy;
            }
            // Whole clicks only: many Windows programs, games included, act
            // only on full 120-unit steps and drop smaller ones.
            Output::Wheel { clicks, .. } => wheel += clicks * crate::mapper::WHEEL_CLICK,
        }
    }
    if mx != 0 || my != 0 {
        inputs.push(mouse_input(MOUSEEVENTF_MOVE, mx, my, 0));
    }
    if wheel != 0 {
        inputs.push(mouse_input(MOUSEEVENTF_WHEEL, 0, 0, wheel));
    }
    if !inputs.is_empty() {
        unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_values() {
        assert_eq!(hat_dir(0, 0, 7), Some(HatDir::Up));
        assert_eq!(hat_dir(2, 0, 7), Some(HatDir::Right));
        assert_eq!(hat_dir(7, 0, 7), Some(HatDir::UpLeft));
        assert_eq!(hat_dir(8, 0, 7), None, "null state is centred");
        assert_eq!(hat_dir(1, 1, 8), Some(HatDir::Up));
        assert_eq!(hat_dir(1, 0, 3), Some(HatDir::Right), "4-way hats");
    }
}
