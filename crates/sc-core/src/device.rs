//! How Star Citizen identifies an input device.
//!
//! The game records each joystick in an `options` element as
//! `Product=" VKBSim Space Gunfighter  L   {0127231D-0000-0000-0000-504944564944}"`:
//! the name Windows reports, then the DirectInput product GUID. The spacing
//! and capitalisation of the name are not stable between files (other tools
//! tidy them), so devices are matched on the GUID plus the name with spacing
//! collapsed and case ignored.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceId {
    /// The name with runs of whitespace collapsed, case kept, for display.
    pub name: String,
    /// The product GUID in upper case with its braces, when the game gave one.
    pub guid: Option<String>,
}

impl DeviceId {
    /// Parse a `Product` attribute.
    pub fn parse(product: &str) -> Self {
        let (name_part, guid) = match (product.rfind('{'), product.rfind('}')) {
            (Some(open), Some(close)) if close > open => {
                (&product[..open], Some(product[open..=close].to_ascii_uppercase()))
            }
            _ => (product, None),
        };
        DeviceId { name: name_part.split_whitespace().collect::<Vec<_>>().join(" "), guid }
    }

    /// The key two records of the same device share.
    pub fn key(&self) -> String {
        format!("{}|{}", self.guid.as_deref().unwrap_or(""), self.name.to_lowercase())
    }

    /// USB vendor and product IDs, for GUIDs of the form
    /// `{PPPPVVVV-0000-0000-0000-504944564944}` (the tail is ASCII "PIDVID").
    pub fn usb_ids(&self) -> Option<(u16, u16)> {
        let inner = self.guid.as_deref()?.trim_start_matches('{').trim_end_matches('}');
        if !inner.ends_with("504944564944") {
            return None;
        }
        let data1 = u32::from_str_radix(inner.get(..8)?, 16).ok()?;
        Some(((data1 & 0xFFFF) as u16, (data1 >> 16) as u16))
    }

    /// The maker's name, for vendors we recognise.
    pub fn vendor(&self) -> Option<&'static str> {
        vendor_name(self.usb_ids()?.0)
    }

    /// A short description of the hardware IDs, e.g. `VKB · 231D:0127`.
    pub fn hardware_label(&self) -> String {
        match self.usb_ids() {
            Some((vid, pid)) => match vendor_name(vid) {
                Some(vendor) => format!("{vendor} · {vid:04X}:{pid:04X}"),
                None => format!("{vid:04X}:{pid:04X}"),
            },
            None => self.guid.clone().unwrap_or_else(|| "no hardware ID".into()),
        }
    }
}

/// USB vendor IDs of makers whose sticks, throttles and pedals turn up in
/// Star Citizen setups. Anything else still works; it just shows the IDs.
fn vendor_name(vid: u16) -> Option<&'static str> {
    Some(match vid {
        0x231D => "VKB",
        0x3344 => "Virpil",
        0x044F => "Thrustmaster",
        0x046D => "Logitech",
        0x06A3 => "Saitek",
        0x4098 => "Winwing",
        0x1DD2 => "Leo Bodnar",
        0x045E => "Microsoft",
        0x054C => "Sony",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_and_guid() {
        let d = DeviceId::parse(" VKBSim Space Gunfighter  L   {0127231D-0000-0000-0000-504944564944}");
        assert_eq!(d.name, "VKBSim Space Gunfighter L");
        assert_eq!(d.guid.as_deref(), Some("{0127231D-0000-0000-0000-504944564944}"));
        assert_eq!(d.usb_ids(), Some((0x231D, 0x0127)));
        assert_eq!(d.hardware_label(), "VKB · 231D:0127");
    }

    #[test]
    fn tidied_names_match_the_original() {
        let game = DeviceId::parse(" VKBsim Gladiator EVO OT L    {3201231D-0000-0000-0000-504944564944}");
        let tidied = DeviceId::parse("VKBsim Gladiator EVO OT L  {3201231d-0000-0000-0000-504944564944}");
        assert_eq!(game.key(), tidied.key());
    }

    #[test]
    fn different_devices_differ() {
        let left = DeviceId::parse(" VKBSim Space Gunfighter  L   {0127231D-0000-0000-0000-504944564944}");
        let right = DeviceId::parse(" VKBSim Space Gunfighter   {0126231D-0000-0000-0000-504944564944}");
        assert_ne!(left.key(), right.key());
    }

    #[test]
    fn devices_without_a_guid() {
        let pad = DeviceId::parse("Controller (Gamepad)");
        assert_eq!(pad.guid, None);
        assert_eq!(pad.usb_ids(), None);
        assert_eq!(pad.name, "Controller (Gamepad)");
    }
}
