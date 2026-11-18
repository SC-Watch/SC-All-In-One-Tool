//! The whole flow on a fake Star Citizen folder: find it, remember the
//! joysticks, have the game swap them, fix, save and restore profiles.

use sc_core::actionmaps::{ActionMaps, Renumber, renumber};
use sc_core::attributes::Group;
use sc_core::data::DataDir;
use sc_core::fsutil::TextFile;
use sc_core::install::{self, Channel};
use sc_core::joysticks::Status;
use sc_core::{ops, profiles};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// `<tmp>/Roberts Space Industries/StarCitizen/LIVE` with the live fixtures.
fn fake_install(tmp: &Path) -> Channel {
    let root = tmp.join("Roberts Space Industries").join("StarCitizen");
    let channel = Channel { name: "LIVE".into(), dir: root.join("LIVE") };
    std::fs::create_dir_all(channel.profile_dir()).unwrap();
    std::fs::copy(fixture_path("actionmaps_live.xml"), channel.actionmaps()).unwrap();
    std::fs::copy(fixture_path("attributes.xml"), channel.attributes()).unwrap();
    std::fs::copy(fixture_path("user.cfg"), channel.user_cfg()).unwrap();
    channel
}

#[test]
fn finds_the_install_from_any_nearby_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let ch = fake_install(tmp.path());
    let root = ch.dir.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(root.join("PTU").join("Bin64")).unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();

    let names: Vec<String> = install::channels(&root).into_iter().map(|c| c.name).collect();
    assert_eq!(names, vec!["LIVE", "PTU"]);
    assert_eq!(install::normalise_root(&root), Some(root.clone()));
    assert_eq!(install::normalise_root(&ch.dir), Some(root.clone()));
    assert_eq!(install::normalise_root(root.parent().unwrap()), Some(root.clone()));
    assert_eq!(install::normalise_root(tmp.path()), None);
}

#[test]
fn folder_names_match_in_any_case() {
    // Windows ignores case, so installs differ (`user\client\0` or
    // `USER\Client\0`); on Linux under Wine the names must still be found.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("StarCitizen");
    let profile = root.join("LIVE").join("user").join("client").join("0").join("Profiles").join("default");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::copy(fixture_path("actionmaps_live.xml"), profile.join("actionmaps.xml")).unwrap();
    std::fs::copy(fixture_path("user.cfg"), root.join("LIVE").join("USER.CFG")).unwrap();

    let channels = install::channels(&root);
    assert_eq!(channels.len(), 1, "found through the lower-case user folder");
    let ch = &channels[0];
    assert_eq!(ch.actionmaps(), profile.join("actionmaps.xml"));
    assert!(ch.actionmaps().is_file());
    assert!(ch.user_cfg().is_file());
    assert_eq!(install::normalise_root(&tmp.path().join("starcitizen")), None, "missing stays missing");
    assert_eq!(install::normalise_root(tmp.path()), Some(root.clone()));

    let data = DataDir::at(tmp.path().join("data"));
    let state = ops::joystick_state(&data, ch);
    assert!(state.problem.is_none(), "{:?}", state.problem);
    assert_eq!(state.live.unwrap().joysticks.len(), 4);
}

#[test]
fn remember_swap_fix() {
    let tmp = tempfile::tempdir().unwrap();
    let data = DataDir::at(tmp.path().join("data"));
    let ch = fake_install(tmp.path());

    // Nothing remembered yet.
    let state = ops::joystick_state(&data, &ch);
    assert!(state.live.is_some() && state.set.is_none());

    ops::remember(&data, &ch).unwrap();
    let state = ops::joystick_state(&data, &ch);
    assert!(state.rows.iter().all(|r| r.status() == Status::Ok));

    // The game hands out new numbers but leaves the bindings where they were.
    let file = TextFile::read(&ch.actionmaps()).unwrap();
    let live = ActionMaps::parse(&file.text).unwrap();
    let options: HashMap<String, u32> =
        live.joysticks.iter().map(|j| (j.device.key(), [0, 1, 3, 4, 2][j.instance as usize])).collect();
    let (swapped, _) = renumber(&file.text, &Renumber { inputs: BTreeMap::new(), options }).unwrap();
    std::fs::write(ch.actionmaps(), &swapped).unwrap();

    let state = ops::joystick_state(&data, &ch);
    assert_eq!(state.rows.iter().filter(|r| r.status() == Status::Moved).count(), 3);

    let dry = ops::fix_joysticks(&data, &ch, &BTreeMap::new(), true).unwrap();
    assert_eq!(dry.plan.moves.len(), 3);
    assert!(!dry.written);
    assert_eq!(std::fs::read_to_string(ch.actionmaps()).unwrap(), swapped, "a dry run writes nothing");

    let done = ops::fix_joysticks(&data, &ch, &BTreeMap::new(), false).unwrap();
    assert!(done.written);
    let backup = done.backup.unwrap();
    assert_eq!(std::fs::read_to_string(backup.join("actionmaps.xml")).unwrap(), swapped);

    let state = ops::joystick_state(&data, &ch);
    assert!(state.rows.iter().all(|r| r.status() == Status::Ok));
    let again = ops::fix_joysticks(&data, &ch, &BTreeMap::new(), false).unwrap();
    assert!(again.plan.is_noop() && !again.written);
}

#[test]
fn numbers_can_be_set_by_hand() {
    // If the game does not record its new order, the user says it instead.
    let tmp = tempfile::tempdir().unwrap();
    let data = DataDir::at(tmp.path().join("data"));
    let ch = fake_install(tmp.path());
    ops::remember(&data, &ch).unwrap();
    let state = ops::joystick_state(&data, &ch);
    let key = |name: &str| state.rows.iter().find(|r| r.device.name == name).unwrap().device.key();
    let overrides = BTreeMap::from([(key("VKBSim T-Rudder"), 4), (key("VKBSim Space Gunfighter"), 2)]);

    let out = ops::fix_joysticks(&data, &ch, &overrides, false).unwrap();
    assert_eq!(out.plan.moves.len(), 2);
    assert_eq!(out.plan.option_changes, 2);
    let after = ActionMaps::parse(&TextFile::read(&ch.actionmaps()).unwrap().text).unwrap();
    assert_eq!(after.bindings_on(4), 1, "the rudder's binding followed it to js4");
    assert_eq!(after.bindings_on(2), 49);
}

#[test]
fn save_and_restore_a_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let data = DataDir::at(tmp.path().join("data"));
    let ch = fake_install(tmp.path());

    // Save September's setup as a profile.
    std::fs::copy(fixture_path("actionmaps_backup_2026-09-03.xml"), ch.actionmaps()).unwrap();
    let saved = profiles::save(&data, "September", &ch, false).unwrap();
    assert!(saved.has_controls() && saved.has_attributes() && saved.has_user_cfg());
    assert!(profiles::save(&data, "september", &ch, false).is_err(), "names are unique");

    // Today the devices have other numbers and the graphics were changed.
    std::fs::copy(fixture_path("actionmaps_live.xml"), ch.actionmaps()).unwrap();
    let attrs = std::fs::read_to_string(ch.attributes()).unwrap();
    std::fs::write(
        ch.attributes(),
        attrs.replace(r#""SysSpec_Shading" value="4""#, r#""SysSpec_Shading" value="1""#),
    )
    .unwrap();

    let profile = profiles::find(&data, "SEPTEMBER").unwrap();
    let opts =
        ops::RestoreOptions { controls: true, user_cfg: true, groups: BTreeSet::from([Group::Graphics]) };
    let out = ops::restore_profile(&data, &profile, &ch, &opts).unwrap();
    assert_eq!(out.controls.unwrap().moves.len(), 3);
    assert_eq!(out.attributes.unwrap().changed, 1);
    assert!(out.backup.unwrap().join("attributes.xml").exists());

    let restored = ActionMaps::parse(&TextFile::read(&ch.actionmaps()).unwrap().text).unwrap();
    let live =
        ActionMaps::parse(&TextFile::read(&fixture_path("actionmaps_live.xml")).unwrap().text).unwrap();
    for j in &restored.joysticks {
        assert_eq!(Some(j.instance), live.instance_of(&j.device), "{} keeps today's number", j.device.name);
    }
    let file = std::fs::read(ch.actionmaps()).unwrap();
    assert!(file.starts_with("\u{feff}".as_bytes()), "the profile file's BOM is kept");
    assert!(std::fs::read_to_string(ch.attributes()).unwrap().contains(r#""SysSpec_Shading" value="4""#));

    // Restoring set the remembered joysticks to match.
    let state = ops::joystick_state(&data, &ch);
    assert!(state.rows.iter().all(|r| r.status() == Status::Ok));

    profiles::delete(&data, "September").unwrap();
    assert!(profiles::list(&data).unwrap().is_empty());
}
