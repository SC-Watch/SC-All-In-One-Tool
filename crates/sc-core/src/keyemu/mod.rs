//! Running a key mapping: reading the sticks and sending keys and mouse
//! movement. The sticks are only read, never taken over, so the game still
//! sees every button press, as with JoyToKey.
//!
//! Linux for now: sticks are read through evdev and keys and mouse movement
//! are sent through a virtual keyboard and mouse (uinput).

use crate::device::DeviceId;
use crate::keymap::{Input, KeyProfile};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

#[cfg(target_os = "linux")]
mod linux;

/// Whether key mapping can run on this system yet.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// A one-line fix for Linux systems where users may not use /dev/uinput:
/// let the logged-in user use it, as Steam and similar tools do.
pub const UINPUT_FIX_COMMAND: &str = "echo 'KERNEL==\"uinput\", SUBSYSTEM==\"misc\", TAG+=\"uaccess\", OPTIONS+=\"static_node=uinput\"' | sudo tee /etc/udev/rules.d/60-sc-aio-uinput.rules";

/// The virtual keyboard and mouse could not be made, nearly always for lack
/// of permission on /dev/uinput.
#[derive(Debug)]
pub struct NoVirtualDevices(pub std::io::Error);

impl std::fmt::Display for NoVirtualDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Could not create the virtual keyboard and mouse ({}). Your user needs permission to use /dev/uinput. \
             One way is to run this in a terminal, then restart the computer:\n{UINPUT_FIX_COMMAND}",
            self.0
        )
    }
}

impl std::error::Error for NoVirtualDevices {}

/// A stick or other controller the system lists.
#[derive(Clone, Debug)]
pub struct LiveStick {
    pub device: DeviceId,
    pub buttons: usize,
    pub hats: usize,
    /// Which stick in the running profile it is, if any.
    pub profile_stick: Option<usize>,
}

/// A press seen on a stick, for showing what the tool reads and for
/// identifying which stick is which.
#[derive(Clone, Debug)]
pub struct Seen {
    pub device: DeviceId,
    pub input: Input,
    pub at: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub running: bool,
    /// Sending keys and mouse movement (false when only watching).
    pub sending: bool,
    pub sticks: Vec<LiveStick>,
    /// Newest first.
    pub recent: VecDeque<Seen>,
    pub error: Option<String>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const RECENT: usize = 12;

impl Status {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn saw(&mut self, seen: Seen) {
        self.recent.push_front(seen);
        self.recent.truncate(RECENT);
    }
}

/// Whether a device the system lists is the one a profile stick names.
/// Matched on USB vendor and product IDs where both have them, since the
/// names Windows, the game and Linux report differ.
pub fn same_device(a: &DeviceId, b: &DeviceId) -> bool {
    match (a.usb_ids(), b.usb_ids()) {
        (Some(x), Some(y)) => x == y,
        _ => a.name.eq_ignore_ascii_case(&b.name),
    }
}

/// A running mapping (or watcher). Stops, letting go of every key, when
/// dropped.
pub struct Runner {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<Status>>,
    thread: Option<JoinHandle<()>>,
}

impl Runner {
    /// Start mapping `profile`'s sticks. With `send` false it only watches
    /// the sticks (no keys are sent and no virtual devices are made), which
    /// is how a stick is identified.
    pub fn start(profile: KeyProfile, send: bool) -> anyhow::Result<Runner> {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(Status { running: true, sending: send, ..Default::default() }));
        #[cfg(target_os = "linux")]
        {
            let (s, st) = (stop.clone(), status.clone());
            let ready = linux::prepare(send)?;
            let thread = std::thread::Builder::new()
                .name("key-mapping".into())
                .spawn(move || linux::run(profile, ready, s, st))?;
            Ok(Runner { stop, status, thread: Some(thread) })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (profile, send, stop, status);
            anyhow::bail!("Key mapping runs on Linux for now; Windows support is the next step.")
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_match_on_usb_ids_not_names() {
        let windows = DeviceId::parse(" VKBSim Space Gunfighter  L   {0127231D-0000-0000-0000-504944564944}");
        let linux = DeviceId::parse(
            "VKB-Sim © Alex Oz 2022 VKBSim Space Gunfighter L {0127231D-0000-0000-0000-504944564944}",
        );
        let other = DeviceId::parse(" VKBSim Space Gunfighter   {0126231D-0000-0000-0000-504944564944}");
        assert!(same_device(&windows, &linux));
        assert!(!same_device(&windows, &other));
    }
}
