//! Reading and writing the game's files without disturbing them.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

const BOM: &str = "\u{feff}";

/// A text file split into an optional UTF-8 byte-order mark and its content,
/// so the mark can be put back exactly as found.
pub struct TextFile {
    pub bom: bool,
    pub text: String,
}

impl TextFile {
    pub fn from_string(raw: String) -> Self {
        match raw.strip_prefix(BOM) {
            Some(rest) => TextFile { bom: true, text: rest.to_string() },
            None => TextFile { bom: false, text: raw },
        }
    }

    pub fn read(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
        Ok(Self::from_string(raw))
    }

    /// The content with the mark restored.
    pub fn with_text(&self, text: String) -> String {
        if self.bom { format!("{BOM}{text}") } else { text }
    }
}

/// Write through a temporary file and a rename, so a crash or a full disk
/// never leaves the game with half a bindings file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".sc-aio-tmp");
    let tmp = std::path::PathBuf::from(tmp);
    fs::write(&tmp, contents).with_context(|| format!("could not write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

/// Make a name safe to use as a folder name on Windows and Linux.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() { "_".into() } else { cleaned }
}

/// A short stable hash (FNV-1a), for naming files after paths.
pub fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_round_trips() {
        let f = TextFile::from_string("\u{feff}<a/>".into());
        assert!(f.bom);
        assert_eq!(f.text, "<a/>");
        assert_eq!(f.with_text("<b/>".into()), "\u{feff}<b/>");
    }

    #[test]
    fn file_names_are_cleaned() {
        assert_eq!(safe_file_name("HOSAS: mining/salvage?"), "HOSAS_ mining_salvage_");
        assert_eq!(safe_file_name("  ..  "), "_");
    }
}
