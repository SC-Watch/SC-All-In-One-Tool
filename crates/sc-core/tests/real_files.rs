//! Tests against real Star Citizen files (see tests/fixtures):
//!
//! * `layout_gunfighter_exported.xml`: a layout exported from the game.
//! * `actionmaps_live.xml`: the live bindings file.
//! * `actionmaps_backup_2026-09-03.xml`: a backup of the same setup from
//!   September, written by another tool (BOM, tidied names), in which three
//!   devices had different numbers.
//! * `attributes.xml` (ship IDs replaced) and `user.cfg`.

use sc_core::actionmaps::{ActionMaps, Renumber, renumber};
use sc_core::attributes::{self, Group};
use sc_core::fsutil::TextFile;
use sc_core::joysticks::{self, JoystickSet, Status};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

fn fixture(name: &str) -> TextFile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    TextFile::read(&path).unwrap()
}

fn parse(name: &str) -> ActionMaps {
    ActionMaps::parse(&fixture(name).text).unwrap()
}

/// Each device's bindings, keyed by device rather than by number.
fn by_device(am: &ActionMaps) -> BTreeMap<String, BTreeSet<String>> {
    am.joysticks
        .iter()
        .map(|j| (j.device.name.clone(), am.fingerprints.get(&j.instance).cloned().unwrap_or_default()))
        .collect()
}

fn numbers(am: &ActionMaps) -> BTreeMap<String, u32> {
    am.joysticks.iter().map(|j| (j.device.name.clone(), j.instance)).collect()
}

const GLADIATOR: &str = "VKBsim Gladiator EVO OT L";
const RUDDER: &str = "VKBSim T-Rudder";
const LEFT: &str = "VKBSim Space Gunfighter L";
const RIGHT: &str = "VKBSim Space Gunfighter";

#[test]
fn reads_the_exported_layout() {
    let am = parse("layout_gunfighter_exported.xml");
    assert_eq!(
        numbers(&am),
        BTreeMap::from([(GLADIATOR.into(), 1), (RUDDER.into(), 2), (LEFT.into(), 3), (RIGHT.into(), 4)])
    );
    assert_eq!(am.bindings_on(1), 8);
    assert_eq!(am.bindings_on(2), 1);
    assert_eq!(am.bindings_on(3), 39);
    assert_eq!(am.bindings_on(4), 49);
    assert_eq!(am.counts[&4].1, 240, "cleared defaults on js4");
    assert_eq!(am.joysticks[0].device.hardware_label(), "VKB · 231D:3201");
}

#[test]
fn reads_the_live_file_and_skips_empty_slots() {
    let am = parse("actionmaps_live.xml");
    assert_eq!(am.joysticks.len(), 4, "instances 5-8 have no device and are ignored");
    assert!(am.device_settings.iter().any(|n| n == "VKBsim Gladiator EVO R"));
    assert!(am.duplicate_keys().is_empty());
}

#[test]
fn reads_the_backup_with_bom_and_tidied_names() {
    let file = fixture("actionmaps_backup_2026-09-03.xml");
    assert!(file.bom);
    let backup = ActionMaps::parse(&file.text).unwrap();
    let live = parse("actionmaps_live.xml");
    for j in &backup.joysticks {
        assert!(live.instance_of(&j.device).is_some(), "{} matches across files", j.device.name);
    }
    assert_eq!(
        numbers(&backup),
        BTreeMap::from([(GLADIATOR.into(), 1), (LEFT.into(), 2), (RIGHT.into(), 3), (RUDDER.into(), 4)])
    );
}

#[test]
fn rewriting_without_changes_keeps_every_byte() {
    for name in ["layout_gunfighter_exported.xml", "actionmaps_live.xml", "actionmaps_backup_2026-09-03.xml"]
    {
        let file = fixture(name);
        let (out, stats) = renumber(&file.text, &Renumber::default()).unwrap();
        assert_eq!(stats.inputs_changed, 0);
        assert!(out == file.text, "{name} changed");
        let original =
            std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name))
                .unwrap();
        assert_eq!(file.with_text(out).into_bytes(), original, "{name} bytes");
    }
}

#[test]
fn a_file_renumbered_consistently_is_not_flagged() {
    // Between September and now the devices got new numbers, and the
    // bindings moved with them. Nothing is wrong, so nothing should be fixed.
    let backup = parse("actionmaps_backup_2026-09-03.xml");
    let live = parse("actionmaps_live.xml");
    let set = JoystickSet::from_actionmaps(&backup);
    let rows = joysticks::analyse(&live, &set);
    assert_eq!(rows.len(), 4);
    for row in &rows {
        assert_eq!(row.status(), Status::Ok, "{}", row.device.name);
        assert_eq!(row.bindings_on, row.now);
    }
    assert!(joysticks::plan(&rows, &live).is_noop());
}

/// The live file as it would look if the game gave the devices new numbers
/// but left the bindings where they were: the case the fix is for.
fn swapped_live() -> (String, ActionMaps) {
    let live = parse("actionmaps_live.xml");
    let to = |name: &str| live.joysticks.iter().find(|j| j.device.name == name).unwrap().device.key();
    let options: HashMap<String, u32> =
        HashMap::from([(to(RUDDER), 3), (to(LEFT), 4), (to(RIGHT), 2), (to(GLADIATOR), 1)]);
    let (text, stats) =
        renumber(&fixture("actionmaps_live.xml").text, &Renumber { inputs: BTreeMap::new(), options })
            .unwrap();
    assert_eq!(stats.options_changed, 3);
    let am = ActionMaps::parse(&text).unwrap();
    (text, am)
}

#[test]
fn a_swap_is_detected_and_fixed() {
    let original = parse("actionmaps_live.xml");
    let set = JoystickSet::from_actionmaps(&original);
    let (swapped_text, swapped) = swapped_live();

    let rows = joysticks::analyse(&swapped, &set);
    let status: BTreeMap<String, Status> = rows.iter().map(|r| (r.device.name.clone(), r.status())).collect();
    assert_eq!(status[GLADIATOR], Status::Ok);
    assert_eq!(status[RUDDER], Status::Moved);
    assert_eq!(status[LEFT], Status::Moved);
    assert_eq!(status[RIGHT], Status::Moved);

    let plan = joysticks::plan(&rows, &swapped);
    assert_eq!(plan.moves.len(), 3);
    assert!(plan.parked.is_empty());
    assert_eq!(plan.renumber.inputs, BTreeMap::from([(1, 1), (2, 3), (3, 4), (4, 2)]));

    let (fixed_text, stats) = renumber(&swapped_text, &plan.renumber).unwrap();
    assert_eq!(stats.options_changed, 0, "the game's numbers are kept");
    assert_eq!(stats.inputs_changed, 1 + 39 + 6 + 49 + 240);
    let fixed = ActionMaps::parse(&fixed_text).unwrap();
    assert_eq!(numbers(&fixed), numbers(&swapped));
    assert_eq!(by_device(&fixed), by_device(&original), "every device has its own bindings again");

    // And a second look finds nothing left to do.
    let rows = joysticks::analyse(&fixed, &set);
    assert!(rows.iter().all(|r| r.status() == Status::Ok));
    assert!(joysticks::plan(&rows, &fixed).is_noop());
}

#[test]
fn an_unplugged_device_is_moved_out_of_the_way() {
    // The left Gunfighter is unplugged and the game gives its number (3) to
    // the rudder. The rudder's bindings move to 3, so the Gunfighter's must
    // go somewhere free rather than be mixed in with them.
    let original = parse("actionmaps_live.xml");
    let set = JoystickSet::from_actionmaps(&original);
    let mut now = original.clone();
    now.joysticks.retain(|j| j.device.name != LEFT);
    now.joysticks.iter_mut().find(|j| j.device.name == RUDDER).unwrap().instance = 3;

    let rows = joysticks::analyse(&now, &set);
    let left = rows.iter().find(|r| r.device.name == LEFT).unwrap();
    assert_eq!(left.status(), Status::NotConnected);

    let plan = joysticks::plan(&rows, &now);
    assert_eq!(plan.renumber.inputs, BTreeMap::from([(1, 1), (2, 3), (3, 2), (4, 4)]));
    assert_eq!(plan.parked, vec![(LEFT.to_string(), 3, 2)]);
}

#[test]
fn identical_devices_are_left_alone() {
    let original = parse("actionmaps_live.xml");
    let set = JoystickSet::from_actionmaps(&original);
    let mut now = original.clone();
    let twin = now.joysticks.iter().find(|j| j.device.name == RIGHT).unwrap().device.clone();
    now.joysticks.iter_mut().find(|j| j.device.name == LEFT).unwrap().device = twin;
    let rows = joysticks::analyse(&now, &set);
    assert!(rows.iter().filter(|r| r.device.name == RIGHT).all(|r| r.status() == Status::Unclear));
}

#[test]
fn restoring_septembers_bindings_onto_todays_numbers() {
    let saved_file = fixture("actionmaps_backup_2026-09-03.xml");
    let saved = ActionMaps::parse(&saved_file.text).unwrap();
    let live = parse("actionmaps_live.xml");

    let rows = joysticks::rows_for_restore(&saved, Some(&live));
    let plan = joysticks::plan(&rows, &saved);
    assert_eq!(plan.moves.len(), 3);
    let (text, _) = renumber(&saved_file.text, &plan.renumber).unwrap();
    let restored = ActionMaps::parse(&text).unwrap();

    assert_eq!(numbers(&restored), numbers(&live), "devices keep today's numbers");
    assert_eq!(by_device(&restored), by_device(&saved), "each device gets its September bindings");

    // Against today's file, only the changes made since September differ.
    let flat = |am: &ActionMaps| -> BTreeSet<String> {
        am.fingerprints.iter().flat_map(|(n, fp)| fp.iter().map(move |f| format!("js{n}:{f}"))).collect()
    };
    let only_restored: Vec<_> = flat(&restored).difference(&flat(&live)).cloned().collect();
    let only_live: Vec<_> = flat(&live).difference(&flat(&restored)).cloned().collect();
    assert_eq!(only_restored, vec!["js4:spaceship_view/v_view_yaw_left/ "]);
    assert_eq!(only_live, vec!["js3:spaceship_view/v_view_yaw_left/ ", "js3:vehicle_driver/v_yaw/x"]);
}

#[test]
fn attributes_groups_on_the_real_file() {
    let file = fixture("attributes.xml");
    let all = attributes::read(&file.text).unwrap();
    assert_eq!(all.len(), 144);
    let group = |n: &str| attributes::group_of(n);
    assert_eq!(group("SysSpec_TextureQuality"), Group::Graphics);
    assert_eq!(group("Width"), Group::Display);
    assert_eq!(group("selectedShipURN_EA_Horde"), Group::Other);
    let count = |g| all.iter().filter(|(n, _)| attributes::group_of(n) == g).count();
    assert_eq!(count(Group::Graphics), 29);
    assert_eq!(count(Group::Display), 8);
}

#[test]
fn restoring_graphics_leaves_everything_else() {
    let live = fixture("attributes.xml").text;
    let saved = live
        .replace(r#"name="SysSpec_Shading" value="4""#, r#"name="SysSpec_Shading" value="2""#)
        .replace(r#"name="FOV" value="54.7682""#, r#"name="FOV" value="70""#)
        .replace(r#"name="AudioMusicVolume" value="0""#, r#"name="AudioMusicVolume" value="0.5""#)
        .replace(
            r#"<Attr name="Width" value="5120"/>"#,
            "<Attr name=\"Width\" value=\"5120\"/>\r\n <Attr name=\"SysSpec_Brand_New\" value=\"1\"/>",
        );
    let groups = BTreeSet::from([Group::Graphics, Group::Display]);
    let (merged, stats) = attributes::merge(&live, &saved, &groups).unwrap();
    assert_eq!(stats.changed, 2);
    assert_eq!(stats.added, 1);

    let get = |xml: &str, key: &str| {
        attributes::read(xml).unwrap().into_iter().find(|(n, _)| n == key).map(|(_, v)| v)
    };
    assert_eq!(get(&merged, "SysSpec_Shading").as_deref(), Some("2"));
    assert_eq!(get(&merged, "FOV").as_deref(), Some("70"));
    assert_eq!(get(&merged, "AudioMusicVolume").as_deref(), Some("0"), "audio was not asked for");
    assert_eq!(get(&merged, "SysSpec_Brand_New").as_deref(), Some("1"));

    // The new setting goes just before the end, in the file's own style.
    let added = " <Attr name=\"SysSpec_Brand_New\" value=\"1\"/>\r\n";
    assert!(merged.contains(&format!("{added}</Attributes>")));
    // Every other line is untouched, CRLF included.
    let without_added = merged.replacen(added, "", 1);
    let changed: Vec<_> = live.split('\n').zip(without_added.split('\n')).filter(|(a, b)| a != b).collect();
    assert_eq!(changed.len(), 2, "{changed:?}");
}
