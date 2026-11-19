//! The keys that can be sent, with their Windows virtual-key code (what
//! JoyToKey stores) and their Linux input event code (what the virtual
//! keyboard sends).

use serde::{Deserialize, Serialize};

/// A key, stored by its name (`"RCtrl"`, `"R"`, `"Slash"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Key(u8);

struct KeyInfo {
    name: &'static str,
    label: &'static str,
    vk: u8,
    linux: u16,
}

macro_rules! keys {
    ($($name:literal, $label:literal, $vk:literal, $linux:literal;)*) => {
        const KEYS: &[KeyInfo] = &[$(KeyInfo { name: $name, label: $label, vk: $vk, linux: $linux },)*];
    };
}

// name, label, Windows VK, Linux KEY_* code
keys! {
    "LCtrl", "Left Ctrl", 0xA2, 29;
    "RCtrl", "Right Ctrl", 0xA3, 97;
    "LShift", "Left Shift", 0xA0, 42;
    "RShift", "Right Shift", 0xA1, 54;
    "LAlt", "Left Alt", 0xA4, 56;
    "RAlt", "Right Alt", 0xA5, 100;
    "LWin", "Left Win", 0x5B, 125;
    "RWin", "Right Win", 0x5C, 126;
    "A", "A", 0x41, 30; "B", "B", 0x42, 48; "C", "C", 0x43, 46; "D", "D", 0x44, 32;
    "E", "E", 0x45, 18; "F", "F", 0x46, 33; "G", "G", 0x47, 34; "H", "H", 0x48, 35;
    "I", "I", 0x49, 23; "J", "J", 0x4A, 36; "K", "K", 0x4B, 37; "L", "L", 0x4C, 38;
    "M", "M", 0x4D, 50; "N", "N", 0x4E, 49; "O", "O", 0x4F, 24; "P", "P", 0x50, 25;
    "Q", "Q", 0x51, 16; "R", "R", 0x52, 19; "S", "S", 0x53, 31; "T", "T", 0x54, 20;
    "U", "U", 0x55, 22; "V", "V", 0x56, 47; "W", "W", 0x57, 17; "X", "X", 0x58, 45;
    "Y", "Y", 0x59, 21; "Z", "Z", 0x5A, 44;
    "0", "0", 0x30, 11; "1", "1", 0x31, 2; "2", "2", 0x32, 3; "3", "3", 0x33, 4;
    "4", "4", 0x34, 5; "5", "5", 0x35, 6; "6", "6", 0x36, 7; "7", "7", 0x37, 8;
    "8", "8", 0x38, 9; "9", "9", 0x39, 10;
    "F1", "F1", 0x70, 59; "F2", "F2", 0x71, 60; "F3", "F3", 0x72, 61; "F4", "F4", 0x73, 62;
    "F5", "F5", 0x74, 63; "F6", "F6", 0x75, 64; "F7", "F7", 0x76, 65; "F8", "F8", 0x77, 66;
    "F9", "F9", 0x78, 67; "F10", "F10", 0x79, 68; "F11", "F11", 0x7A, 87; "F12", "F12", 0x7B, 88;
    "F13", "F13", 0x7C, 183; "F14", "F14", 0x7D, 184; "F15", "F15", 0x7E, 185; "F16", "F16", 0x7F, 186;
    "F17", "F17", 0x80, 187; "F18", "F18", 0x81, 188; "F19", "F19", 0x82, 189; "F20", "F20", 0x83, 190;
    "F21", "F21", 0x84, 191; "F22", "F22", 0x85, 192; "F23", "F23", 0x86, 193; "F24", "F24", 0x87, 194;
    "Esc", "Esc", 0x1B, 1;
    "Tab", "Tab", 0x09, 15;
    "CapsLock", "Caps Lock", 0x14, 58;
    "Space", "Space", 0x20, 57;
    "Enter", "Enter", 0x0D, 28;
    "Backspace", "Backspace", 0x08, 14;
    "Insert", "Insert", 0x2D, 110;
    "Delete", "Delete", 0x2E, 111;
    "Home", "Home", 0x24, 102;
    "End", "End", 0x23, 107;
    "PageUp", "Page Up", 0x21, 104;
    "PageDown", "Page Down", 0x22, 109;
    "Up", "Up", 0x26, 103;
    "Down", "Down", 0x28, 108;
    "Left", "Left", 0x25, 105;
    "Right", "Right", 0x27, 106;
    "PrintScreen", "Print Screen", 0x2C, 99;
    "ScrollLock", "Scroll Lock", 0x91, 70;
    "Pause", "Pause", 0x13, 119;
    "Minus", "-", 0xBD, 12;
    "Equals", "=", 0xBB, 13;
    "LBracket", "[", 0xDB, 26;
    "RBracket", "]", 0xDD, 27;
    "Backslash", "\\", 0xDC, 43;
    "Semicolon", ";", 0xBA, 39;
    "Apostrophe", "'", 0xDE, 40;
    "Grave", "`", 0xC0, 41;
    "Comma", ",", 0xBC, 51;
    "Period", ".", 0xBE, 52;
    "Slash", "/", 0xBF, 53;
    "NumLock", "Num Lock", 0x90, 69;
    "Num0", "Num 0", 0x60, 82; "Num1", "Num 1", 0x61, 79; "Num2", "Num 2", 0x62, 80;
    "Num3", "Num 3", 0x63, 81; "Num4", "Num 4", 0x64, 75; "Num5", "Num 5", 0x65, 76;
    "Num6", "Num 6", 0x66, 77; "Num7", "Num 7", 0x67, 71; "Num8", "Num 8", 0x68, 72;
    "Num9", "Num 9", 0x69, 73;
    "NumMultiply", "Num *", 0x6A, 55;
    "NumAdd", "Num +", 0x6B, 78;
    "NumSubtract", "Num -", 0x6D, 74;
    "NumDecimal", "Num .", 0x6E, 83;
    "NumDivide", "Num /", 0x6F, 98;
}

impl Key {
    fn info(self) -> &'static KeyInfo {
        &KEYS[self.0 as usize]
    }

    /// Every key, in table order.
    pub fn all() -> impl Iterator<Item = Key> {
        (0..KEYS.len() as u8).map(Key)
    }

    /// From a Windows virtual-key code. The generic Shift, Ctrl and Alt
    /// codes are taken as the left-hand keys.
    pub fn from_vk(vk: u8) -> Option<Key> {
        let vk = match vk {
            0x10 => 0xA0,
            0x11 => 0xA2,
            0x12 => 0xA4,
            other => other,
        };
        KEYS.iter().position(|k| k.vk == vk).map(|i| Key(i as u8))
    }

    /// From a name, ignoring case (`"rctrl"`, `"R"`, `"slash"`), or a
    /// label (`"/"`).
    pub fn from_name(s: &str) -> Option<Key> {
        let s = s.trim();
        KEYS.iter()
            .position(|k| k.name.eq_ignore_ascii_case(s) || k.label.eq_ignore_ascii_case(s))
            .map(|i| Key(i as u8))
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }
    pub fn label(self) -> &'static str {
        self.info().label
    }
    pub fn vk(self) -> u8 {
        self.info().vk
    }
    pub fn linux_code(self) -> u16 {
        self.info().linux
    }
}

impl TryFrom<String> for Key {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        Key::from_name(&s).ok_or_else(|| format!("unknown key \"{s}\""))
    }
}

impl From<Key> for String {
    fn from(k: Key) -> String {
        k.name().to_string()
    }
}

/// Parse `"RCtrl+R"` into keys.
pub fn parse_combo(s: &str) -> Result<Vec<Key>, String> {
    let keys: Result<Vec<Key>, String> = s
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            Key::from_name(p).ok_or_else(|| {
                format!(
                    "Unknown key \"{p}\". Key names look like RCtrl, LShift, RAlt, A, 5, F1, Num5 or Slash."
                )
            })
        })
        .collect();
    let keys = keys?;
    if keys.is_empty() {
        return Err("Type the keys to hold, such as RCtrl or RCtrl+R.".into());
    }
    if keys.len() > 4 {
        return Err("At most 4 keys at once.".into());
    }
    Ok(keys)
}

/// `"Right Ctrl + R"`.
pub fn combo_label(keys: &[Key]) -> String {
    keys.iter().map(|k| k.label()).collect::<Vec<_>>().join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_no_duplicates() {
        let mut names = std::collections::HashSet::new();
        let mut vks = std::collections::HashSet::new();
        let mut codes = std::collections::HashSet::new();
        for k in Key::all() {
            assert!(names.insert(k.name().to_lowercase()), "name {}", k.name());
            assert!(vks.insert(k.vk()), "vk {:#x}", k.vk());
            assert!(codes.insert(k.linux_code()), "linux {}", k.linux_code());
        }
    }

    #[test]
    fn joytokey_codes() {
        assert_eq!(Key::from_vk(0xA3).unwrap().name(), "RCtrl");
        assert_eq!(Key::from_vk(0x52).unwrap().name(), "R");
        assert_eq!(Key::from_vk(0xA5).unwrap().name(), "RAlt");
        assert_eq!(Key::from_vk(0xBF).unwrap().name(), "Slash");
        assert_eq!(Key::from_vk(0x11).unwrap().name(), "LCtrl");
        assert_eq!(Key::from_vk(0xFF), None);
    }

    #[test]
    fn combos() {
        let keys = parse_combo("rctrl + r").unwrap();
        assert_eq!(combo_label(&keys), "Right Ctrl + R");
        assert_eq!(
            parse_combo("RAlt+/").unwrap(),
            vec![Key::from_name("RAlt").unwrap(), Key::from_name("Slash").unwrap()]
        );
        assert!(parse_combo("Hyper+X").is_err());
        assert!(parse_combo("").is_err());
    }

    #[test]
    fn keys_round_trip_through_toml() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct T {
            keys: Vec<Key>,
        }
        let t = T { keys: parse_combo("RCtrl+R").unwrap() };
        let s = toml::to_string(&t).unwrap();
        assert_eq!(s.trim(), r#"keys = ["RCtrl", "R"]"#);
        assert_eq!(toml::from_str::<T>(&s).unwrap(), t);
    }
}
