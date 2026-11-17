//! Copies of the game's files, taken before every change.

use crate::data::DataDir;
use crate::install::Channel;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// How many backups to keep; older ones are removed.
const KEEP: usize = 40;

/// Copy whichever of the channel's settings files exist into a new
/// timestamped folder, and return it.
pub fn backup_channel(data: &DataDir, channel: &Channel, reason: &str) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S");
    let base = data.backups_dir();
    let label = format!(
        "{stamp}_{}_{}",
        crate::fsutil::safe_file_name(&channel.name),
        crate::fsutil::safe_file_name(reason)
    );
    let mut dir = base.join(&label);
    let mut n = 2;
    while dir.exists() {
        dir = base.join(format!("{label}-{n}"));
        n += 1;
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    for file in [channel.actionmaps(), channel.attributes(), channel.user_cfg()] {
        if file.exists() {
            let name = file.file_name().expect("has a name");
            std::fs::copy(&file, dir.join(name))
                .with_context(|| format!("could not back up {}", file.display()))?;
        }
    }
    prune(&base);
    Ok(dir)
}

fn prune(base: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(base) else { return };
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    if dirs.len() <= KEEP {
        return;
    }
    dirs.sort(); // names start with the timestamp
    for old in &dirs[..dirs.len() - KEEP] {
        let _ = std::fs::remove_dir_all(old);
    }
}
