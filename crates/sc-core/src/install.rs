//! Finding Star Citizen and its game versions (LIVE, PTU, ...).
//!
//! On Windows the game is usually under `Roberts Space Industries\StarCitizen`
//! on some drive. On Linux it runs under Wine, inside a prefix such as the one
//! the LUG helper and Lutris create at `~/Games/star-citizen`.

use std::path::{Path, PathBuf};

/// One game version folder, e.g. `...\StarCitizen\LIVE`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub name: String,
    pub dir: PathBuf,
}

impl Channel {
    pub fn profile_dir(&self) -> PathBuf {
        self.dir.join("USER").join("Client").join("0").join("Profiles").join("default")
    }
    pub fn actionmaps(&self) -> PathBuf {
        self.profile_dir().join("actionmaps.xml")
    }
    pub fn attributes(&self) -> PathBuf {
        self.profile_dir().join("attributes.xml")
    }
    pub fn user_cfg(&self) -> PathBuf {
        self.dir.join("user.cfg")
    }
    /// Where the game keeps layouts exported from its keybinding screen.
    pub fn mappings_dir(&self) -> PathBuf {
        self.dir.join("USER").join("Client").join("0").join("Controls").join("Mappings")
    }
}

const KNOWN_ORDER: [&str; 5] = ["LIVE", "PTU", "EPTU", "TECH-PREVIEW", "HOTFIX"];

/// The game versions installed under a `StarCitizen` folder.
pub fn channels(root: &Path) -> Vec<Channel> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<Channel> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            let p = e.path();
            p.join("USER").is_dir() || p.join("Bin64").is_dir() || p.join("Data.p4k").is_file()
        })
        .map(|e| Channel { name: e.file_name().to_string_lossy().into_owned(), dir: e.path() })
        .collect();
    found.sort_by_key(|c| {
        let rank =
            KNOWN_ORDER.iter().position(|k| k.eq_ignore_ascii_case(&c.name)).unwrap_or(KNOWN_ORDER.len());
        (rank, c.name.to_lowercase())
    });
    found
}

/// Accept the `StarCitizen` folder, a version folder inside it (`LIVE`), or
/// the `Roberts Space Industries` folder above it, and return the
/// `StarCitizen` folder.
pub fn normalise_root(picked: &Path) -> Option<PathBuf> {
    if !channels(picked).is_empty() {
        return Some(picked.to_path_buf());
    }
    if let Some(parent) = picked.parent()
        && channels(parent).iter().any(|c| c.dir == picked)
    {
        return Some(parent.to_path_buf());
    }
    let below = picked.join("StarCitizen");
    if !channels(&below).is_empty() {
        return Some(below);
    }
    None
}

/// Look in the usual places.
pub fn detect() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates() {
        if !channels(&candidate).is_empty() && !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    found
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        for drive in 'C'..='Z' {
            let d = PathBuf::from(format!("{drive}:\\"));
            for sub in [
                "Program Files\\Roberts Space Industries\\StarCitizen",
                "Roberts Space Industries\\StarCitizen",
                "Games\\Roberts Space Industries\\StarCitizen",
                "StarCitizen",
            ] {
                out.push(d.join(sub));
            }
        }
    } else {
        out.extend(wine_prefixes().into_iter().map(|p| {
            p.join("drive_c").join("Program Files").join("Roberts Space Industries").join("StarCitizen")
        }));
    }
    out
}

/// Wine prefixes Star Citizen is commonly installed into on Linux.
fn wine_prefixes() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("WINEPREFIX") {
        out.push(PathBuf::from(p));
    }
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    // The LUG helper's and Lutris's default, then anything else under ~/Games.
    out.push(home.join("Games").join("star-citizen"));
    out.extend(subdirs(&home.join("Games")));
    out.push(home.join(".wine"));
    // Bottles, as a Flatpak and natively.
    out.extend(subdirs(&home.join(".var/app/com.usebottles.bottles/data/bottles/bottles")));
    out.extend(subdirs(&home.join(".local/share/bottles/bottles")));
    out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}
