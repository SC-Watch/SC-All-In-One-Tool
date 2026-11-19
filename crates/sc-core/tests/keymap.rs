//! Importing a real JoyToKey profile (tests/fixtures/joytokey_gunfighter_iv_dual.cfg)
//! and saving key mappings.

use sc_core::data::DataDir;
use sc_core::keymap::{self, Action, HatDir, Input};
use std::path::PathBuf;

fn fixture() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/joytokey_gunfighter_iv_dual.cfg");
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn imports_the_gunfighter_profile() {
    let import = keymap::import_joytokey("Gunfighter IV Dual", &fixture()).unwrap();
    assert!(import.skipped.is_empty(), "{:?}", import.skipped);
    let p = &import.profile;
    assert_eq!(p.mapping_count(), 12);
    // Stick 4 has nothing mapped, so it is dropped.
    let numbers: Vec<u8> = p.sticks.iter().map(|s| s.joytokey_number.unwrap()).collect();
    assert_eq!(numbers, vec![1, 2, 3]);
    assert!(p.sticks.iter().all(|s| s.device.is_none()), "the file does not say which stick is which");

    let labels = |n: u8| -> Vec<String> {
        p.sticks
            .iter()
            .find(|s| s.joytokey_number == Some(n))
            .unwrap()
            .mappings
            .iter()
            .map(|m| format!("{} -> {}", m.input.label(), m.action.label()))
            .collect()
    };
    assert_eq!(
        labels(1),
        vec![
            "Hat 1 up -> Move mouse up",
            "Hat 1 right -> Move mouse right",
            "Hat 1 down -> Move mouse down",
            "Hat 1 left -> Move mouse left",
        ]
    );
    assert_eq!(
        labels(2),
        vec!["Button 5 -> Right Ctrl + R", "Button 21 -> Scroll up", "Button 23 -> Scroll down"]
    );
    assert_eq!(labels(3)[4], "Button 4 -> Right Alt + /");

    let stick1 = &p.sticks[0];
    assert_eq!(stick1.mappings[0].input, Input::Hat { hat: 1, direction: HatDir::Up });
    assert_eq!(stick1.mappings[0].action, Action::Mouse { dx: 0, dy: -100 });
}

#[test]
fn reports_what_it_cannot_import() {
    let cfg = "[General]\nFileVersion=70\n\n[Joystick 1]\nButton01=6, \"Launcher\", 0\nButton02=1, FF:00:00:00, 0.000, 0, 0\nAxis1n=1, 41:00:00:00, 0.000, 0, 0\nButton03=0, 00:00:00:00\n";
    let import = keymap::import_joytokey("x", cfg).unwrap();
    assert_eq!(import.profile.mapping_count(), 0);
    assert_eq!(import.skipped.len(), 3, "{:?}", import.skipped);
    assert!(keymap::import_joytokey("x", "[General]\nFileVersion=70\n").is_err());
    assert!(keymap::import_joytokey("x", "just some text").is_err());
}

#[test]
fn saves_lists_and_deletes() {
    let tmp = tempfile::tempdir().unwrap();
    let data = DataDir::at(tmp.path());
    let p = keymap::import_joytokey("Gunfighter IV Dual", &fixture()).unwrap().profile;
    keymap::save(&data, &p).unwrap();
    let all = keymap::list(&data).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], p);
    assert_eq!(keymap::load(&data, "gunfighter iv dual").unwrap(), p);
    keymap::delete(&data, "Gunfighter IV Dual").unwrap();
    assert!(keymap::list(&data).unwrap().is_empty());
}
