//! Where the tool keeps its own things: settings, profiles, remembered
//! joysticks and backups.
//!
//! Portable by default, like the release folder it ships in: if
//! `portable.txt` sits next to the program, everything goes in `data` beside
//! it, so the whole tool is one folder you can move or copy. Without that
//! file it uses the usual per-user folder (`%APPDATA%\sc-aio` on Windows,
//! `~/.local/share/sc-aio` on Linux).

use crate::launch::LaunchItem;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PORTABLE_MARKER: &str = "portable.txt";
const APP_DIR: &str = "sc-aio";

#[derive(Clone, Debug)]
pub struct DataDir {
    pub root: PathBuf,
    pub portable: bool,
}

impl DataDir {
    /// Work out where data lives for this run.
    pub fn locate() -> Result<Self> {
        if let Some(dir) = std::env::var_os("SC_AIO_DATA") {
            return Ok(DataDir { root: PathBuf::from(dir), portable: true });
        }
        if let Some(dir) = program_dir()
            && dir.join(PORTABLE_MARKER).exists()
        {
            return Ok(DataDir { root: dir.join("data"), portable: true });
        }
        let base = dirs::data_dir().context("could not find your user data folder")?;
        Ok(DataDir { root: base.join(APP_DIR), portable: false })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        DataDir { root: root.into(), portable: true }
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("settings.toml")
    }
    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }
    /// The remembered joystick setup for one game version.
    pub fn joystick_file(&self, channel: &crate::install::Channel) -> PathBuf {
        let id = crate::fsutil::short_hash(&channel.dir.to_string_lossy());
        self.root
            .join("joysticks")
            .join(format!("{}-{id}.toml", crate::fsutil::safe_file_name(&channel.name)))
    }
}

/// The folder the user sees the program in. For an AppImage that is the
/// folder holding the `.AppImage` file, not its read-only mount.
fn program_dir() -> Option<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Path::new(&appimage).parent().map(Path::to_path_buf);
    }
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Settings the user chooses.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `StarCitizen` folders added by hand.
    pub installs: Vec<PathBuf>,
    /// The game version last picked.
    pub selected_install: Option<PathBuf>,
    pub selected_channel: Option<String>,
    /// Programs to start with "Launch all".
    pub launch: Vec<LaunchItem>,
    /// The key mapping in use.
    pub key_mapping: Option<String>,
    /// Start key mapping when the app opens.
    pub key_mapping_autostart: bool,
}

impl Config {
    pub fn load(data: &DataDir) -> Result<Self> {
        let path = data.config_file();
        if !path.exists() {
            return Ok(Config::default());
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("{} could not be read", path.display()))
    }

    pub fn save(&self, data: &DataDir) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        crate::fsutil::write_atomic(&data.config_file(), text.as_bytes())
    }
}
