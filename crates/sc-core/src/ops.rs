//! The actions the app and the command line offer, built from the pieces in
//! the other modules. Every action that writes to the game's files checks
//! the game is closed and takes a backup first.

use crate::actionmaps::{ActionMaps, renumber};
use crate::attributes::{self, Group, MergeStats};
use crate::backup::backup_channel;
use crate::data::DataDir;
use crate::fsutil::{TextFile, write_atomic};
use crate::install::Channel;
use crate::joysticks::{self, DeviceRow, FixPlan, JoystickSet};
use crate::profiles::Profile;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Every `StarCitizen` folder to offer: ones added by hand, then ones found.
pub fn installs(config: &crate::data::Config) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in config.installs.iter().cloned().chain(crate::install::detect()) {
        if !out.contains(&p) && !crate::install::channels(&p).is_empty() {
            out.push(p);
        }
    }
    out
}

/// Choose the game version to work on: the one asked for, else the one last
/// picked, else LIVE in the first install found.
pub fn select_channel(
    config: &crate::data::Config,
    install: Option<&std::path::Path>,
    channel: Option<&str>,
) -> Result<Channel> {
    let root = match install {
        Some(p) => crate::install::normalise_root(p)
            .with_context(|| format!("no Star Citizen game versions found in {}", p.display()))?,
        None => {
            let all = installs(config);
            match &config.selected_install {
                Some(sel) if all.contains(sel) => sel.clone(),
                _ => all.into_iter().next().context(
                    "Star Citizen was not found. Add its folder in Settings, or pass --install <folder>.",
                )?,
            }
        }
    };
    let channels = crate::install::channels(&root);
    let wanted = channel.map(str::to_string).or_else(|| config.selected_channel.clone());
    if let Some(name) = wanted {
        if let Some(c) = channels.iter().find(|c| c.name.eq_ignore_ascii_case(&name)) {
            return Ok(c.clone());
        }
        if channel.is_some() {
            let names: Vec<_> = channels.iter().map(|c| c.name.as_str()).collect();
            bail!("no {name} folder in {}; found {}", root.display(), names.join(", "));
        }
    }
    channels.into_iter().next().context("no game versions found")
}

/// What the Joysticks page shows for one game version.
#[derive(Clone, Debug, Default)]
pub struct JoystickState {
    pub live: Option<ActionMaps>,
    pub set: Option<JoystickSet>,
    pub rows: Vec<DeviceRow>,
    /// Why the bindings file could not be read, if it could not.
    pub problem: Option<String>,
}

pub fn joystick_state(data: &DataDir, channel: &Channel) -> JoystickState {
    let mut state = JoystickState::default();
    let path = channel.actionmaps();
    if !path.exists() {
        state.problem = Some(format!(
            "{} has no bindings file yet. Start the game once and open the keybinding screen.",
            channel.name
        ));
        return state;
    }
    match TextFile::read(&path).and_then(|f| ActionMaps::parse(&f.text)) {
        Ok(live) => state.live = Some(live),
        Err(e) => state.problem = Some(format!("{e:#}")),
    }
    match JoystickSet::load(&data.joystick_file(channel)) {
        Ok(set) => state.set = set,
        Err(e) => state.problem = Some(format!("{e:#}")),
    }
    if let (Some(live), Some(set)) = (&state.live, &state.set) {
        state.rows = joysticks::analyse(live, set);
    }
    state
}

/// Remember the joystick setup in the live bindings file as the right one.
pub fn remember(data: &DataDir, channel: &Channel) -> Result<JoystickSet> {
    let file = TextFile::read(&channel.actionmaps())?;
    let live = ActionMaps::parse(&file.text)?;
    if live.joysticks.is_empty() {
        bail!("the bindings file lists no joysticks. Start the game with them plugged in first.");
    }
    let set = JoystickSet::from_actionmaps(&live);
    set.save(&data.joystick_file(channel))?;
    Ok(set)
}

#[derive(Clone, Debug)]
pub struct FixOutcome {
    pub plan: FixPlan,
    pub backup: Option<PathBuf>,
    pub written: bool,
}

/// Move each device's bindings to the number the game gives it now.
/// `overrides` sets "number the game uses now" by hand, by device key.
pub fn fix_joysticks(
    data: &DataDir,
    channel: &Channel,
    overrides: &BTreeMap<String, u32>,
    dry_run: bool,
) -> Result<FixOutcome> {
    let file = TextFile::read(&channel.actionmaps())?;
    let live = ActionMaps::parse(&file.text)?;
    let set = JoystickSet::load(&data.joystick_file(channel))?
        .context("no joystick setup is remembered for this game version yet")?;
    let mut rows = joysticks::analyse(&live, &set);
    for row in &mut rows {
        if let Some(&n) = overrides.get(&row.device.key()) {
            row.now = Some(n);
        }
    }
    let plan = joysticks::plan(&rows, &live);
    if plan.is_noop() || dry_run {
        return Ok(FixOutcome { plan, backup: None, written: false });
    }
    crate::game::ensure_closed()?;
    let backup = backup_channel(data, channel, "fix-joysticks")?;
    let (text, _) = renumber(&file.text, &plan.renumber)?;
    write_atomic(&channel.actionmaps(), file.with_text(text.clone()).as_bytes())?;
    // The bindings now match the current numbers: remember that.
    JoystickSet::from_actionmaps(&ActionMaps::parse(&text)?).save(&data.joystick_file(channel))?;
    Ok(FixOutcome { plan, backup: Some(backup), written: true })
}

#[derive(Clone, Debug)]
pub struct RestoreOptions {
    pub controls: bool,
    pub user_cfg: bool,
    pub groups: BTreeSet<Group>,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        RestoreOptions { controls: true, user_cfg: true, groups: Group::DEFAULT.into_iter().collect() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RestoreOutcome {
    pub backup: Option<PathBuf>,
    /// How the profile's bindings were renumbered to fit today's devices.
    pub controls: Option<FixPlan>,
    pub attributes: Option<MergeStats>,
    pub user_cfg: bool,
}

/// Put a profile's settings back into a game version. Bindings are
/// renumbered to the numbers the game gives each device now.
pub fn restore_profile(
    data: &DataDir,
    profile: &Profile,
    channel: &Channel,
    opts: &RestoreOptions,
) -> Result<RestoreOutcome> {
    let do_controls = opts.controls && profile.has_controls();
    let do_attrs = !opts.groups.is_empty() && profile.has_attributes();
    let do_cfg = opts.user_cfg && profile.has_user_cfg();
    if !(do_controls || do_attrs || do_cfg) {
        bail!("nothing to restore: the profile has none of the parts chosen");
    }
    crate::game::ensure_closed()?;
    let mut out = RestoreOutcome {
        backup: Some(backup_channel(data, channel, &format!("restore-{}", profile.name()))?),
        ..Default::default()
    };

    if do_controls {
        let saved_file = TextFile::read(&profile.controls())?;
        let saved = ActionMaps::parse(&saved_file.text)?;
        let live = match channel.actionmaps().exists() {
            true => Some(ActionMaps::parse(&TextFile::read(&channel.actionmaps())?.text)?),
            false => None,
        };
        let rows = joysticks::rows_for_restore(&saved, live.as_ref());
        let plan = joysticks::plan(&rows, &saved);
        let (text, _) = renumber(&saved_file.text, &plan.renumber)?;
        write_atomic(&channel.actionmaps(), saved_file.with_text(text.clone()).as_bytes())?;
        JoystickSet::from_actionmaps(&ActionMaps::parse(&text)?).save(&data.joystick_file(channel))?;
        out.controls = Some(plan);
    }

    if do_attrs {
        let saved = TextFile::read(&profile.attributes())?;
        let target = channel.attributes();
        if target.exists() {
            let live = TextFile::read(&target)?;
            let (text, stats) = attributes::merge(&live.text, &saved.text, &opts.groups)?;
            write_atomic(&target, live.with_text(text).as_bytes())?;
            out.attributes = Some(stats);
        } else {
            std::fs::create_dir_all(channel.profile_dir())?;
            std::fs::copy(profile.attributes(), &target)
                .with_context(|| format!("could not write {}", target.display()))?;
            out.attributes = Some(MergeStats::default());
        }
    }

    if do_cfg {
        let bytes = std::fs::read(profile.user_cfg())?;
        write_atomic(&channel.user_cfg(), &bytes)?;
        out.user_cfg = true;
    }
    Ok(out)
}
