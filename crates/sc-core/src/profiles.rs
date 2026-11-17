//! Named snapshots of a game version's settings: bindings (with each
//! device's saturation, deadzones, curves and invert settings),
//! `attributes.xml` and `user.cfg`.

use crate::data::DataDir;
use crate::fsutil::{safe_file_name, write_atomic};
use crate::install::Channel;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const META: &str = "profile.toml";
pub const CONTROLS_FILE: &str = "actionmaps.xml";
pub const ATTRIBUTES_FILE: &str = "attributes.xml";
pub const USER_CFG_FILE: &str = "user.cfg";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub name: String,
    pub created: String,
    /// The game version it was saved from.
    pub from_channel: String,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub meta: ProfileMeta,
    pub dir: PathBuf,
}

impl Profile {
    pub fn name(&self) -> &str {
        &self.meta.name
    }
    pub fn controls(&self) -> PathBuf {
        self.dir.join(CONTROLS_FILE)
    }
    pub fn attributes(&self) -> PathBuf {
        self.dir.join(ATTRIBUTES_FILE)
    }
    pub fn user_cfg(&self) -> PathBuf {
        self.dir.join(USER_CFG_FILE)
    }
    pub fn has_controls(&self) -> bool {
        self.controls().is_file()
    }
    pub fn has_attributes(&self) -> bool {
        self.attributes().is_file()
    }
    pub fn has_user_cfg(&self) -> bool {
        self.user_cfg().is_file()
    }
}

pub fn list(data: &DataDir) -> Result<Vec<Profile>> {
    let dir = data.profiles_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("could not read {}", dir.display()))? {
        let path = entry?.path();
        let meta_path = path.join(META);
        let Ok(text) = std::fs::read_to_string(&meta_path) else { continue };
        if let Ok(meta) = toml::from_str::<ProfileMeta>(&text) {
            out.push(Profile { meta, dir: path });
        }
    }
    out.sort_by_key(|p| p.meta.name.to_lowercase());
    Ok(out)
}

pub fn find(data: &DataDir, name: &str) -> Result<Profile> {
    list(data)?
        .into_iter()
        .find(|p| p.meta.name.eq_ignore_ascii_case(name.trim()))
        .with_context(|| format!("there is no profile called \"{}\"", name.trim()))
}

/// Save the channel's current settings under `name`.
pub fn save(data: &DataDir, name: &str, channel: &Channel, overwrite: bool) -> Result<Profile> {
    let name = name.trim();
    if name.is_empty() {
        bail!("give the profile a name");
    }
    let sources = [
        (channel.actionmaps(), CONTROLS_FILE),
        (channel.attributes(), ATTRIBUTES_FILE),
        (channel.user_cfg(), USER_CFG_FILE),
    ];
    if !sources.iter().any(|(p, _)| p.is_file()) {
        bail!(
            "{} has no settings files yet. Start the game once and change a setting, then save.",
            channel.name
        );
    }
    // Names are unique ignoring case, on Linux as on Windows.
    if let Ok(existing) = find(data, name) {
        if !overwrite {
            bail!("a profile called \"{}\" already exists", existing.meta.name);
        }
        std::fs::remove_dir_all(&existing.dir)
            .with_context(|| format!("could not replace {}", existing.dir.display()))?;
    }
    let mut dir = data.profiles_dir().join(safe_file_name(name));
    let mut n = 2;
    while dir.exists() {
        // Two names can clean up to the same folder name ("a/b" and "a_b").
        dir = data.profiles_dir().join(format!("{}-{n}", safe_file_name(name)));
        n += 1;
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    for (src, file) in sources {
        if src.is_file() {
            std::fs::copy(&src, dir.join(file))
                .with_context(|| format!("could not copy {}", src.display()))?;
        }
    }
    let meta = ProfileMeta {
        name: name.to_string(),
        created: crate::now_string(),
        from_channel: channel.name.clone(),
    };
    write_atomic(&dir.join(META), toml::to_string_pretty(&meta)?.as_bytes())?;
    Ok(Profile { meta, dir })
}

pub fn delete(data: &DataDir, name: &str) -> Result<()> {
    let profile = find(data, name)?;
    std::fs::remove_dir_all(&profile.dir)
        .with_context(|| format!("could not delete {}", profile.dir.display()))
}
