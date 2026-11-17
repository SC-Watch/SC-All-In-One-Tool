//! "Launch with": programs to start alongside the game, such as TrackIR,
//! JoyToKey or OBS.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaunchItem {
    pub name: String,
    /// A program, a shortcut (`.lnk` on Windows), or a link such as
    /// `steam://...`.
    pub target: String,
    #[serde(default)]
    pub args: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl LaunchItem {
    pub fn new(target: &str) -> Self {
        // Split on both separators: a Windows path is just as likely here.
        let file = target.rsplit(['/', '\\']).next().unwrap_or(target);
        let stem = match file.rfind('.') {
            Some(dot) if dot > 0 => &file[..dot],
            _ => file,
        };
        let name = if stem.is_empty() { target.to_string() } else { stem.to_string() };
        LaunchItem { name, target: target.to_string(), args: String::new(), enabled: true }
    }
}

fn is_link(target: &str) -> bool {
    target.contains("://")
}

/// Start one item without waiting for it.
pub fn launch(item: &LaunchItem) -> Result<()> {
    let target = item.target.trim();
    if target.is_empty() {
        anyhow::bail!("{} has nothing to start", item.name);
    }
    let mut cmd = build_command(target, &item.args);
    let mut child = cmd.spawn().with_context(|| format!("could not start {}", item.name))?;
    // Reap it when it exits, so it does not linger as a zombie on Linux.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Start every enabled item; report the ones that failed.
pub fn launch_all(items: &[LaunchItem]) -> Vec<(String, String)> {
    items
        .iter()
        .filter(|i| i.enabled)
        .filter_map(|i| launch(i).err().map(|e| (i.name.clone(), format!("{e:#}"))))
        .collect()
}

#[cfg(windows)]
fn build_command(target: &str, args: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let is_exe = target.to_ascii_lowercase().ends_with(".exe");
    if is_exe && !is_link(target) {
        let mut cmd = Command::new(target);
        cmd.args(split_args(args));
        if let Some(dir) = Path::new(target).parent().filter(|d| d.is_dir()) {
            cmd.current_dir(dir);
        }
        cmd
    } else {
        // Shortcuts and links go through `start`, as a batch file would.
        let mut cmd = Command::new("cmd");
        cmd.raw_arg(format!("/C start \"\" \"{target}\" {args}"));
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
}

#[cfg(not(windows))]
fn build_command(target: &str, args: &str) -> Command {
    if is_link(target) {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        return cmd;
    }
    let mut cmd = Command::new(target);
    cmd.args(split_args(args));
    if let Some(dir) = Path::new(target).parent().filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    cmd
}

/// Split an argument string on spaces, keeping "quoted parts" together.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_arguments() {
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("--a  b"), vec!["--a", "b"]);
        assert_eq!(split_args(r#"--profile "My Profile" -x"#), vec!["--profile", "My Profile", "-x"]);
        assert_eq!(split_args(r#""""#), vec![""]);
    }

    #[test]
    fn names_come_from_the_file() {
        assert_eq!(LaunchItem::new(r"C:\Program Files (x86)\TrackIR5\TrackIR5.exe").name, "TrackIR5");
        assert_eq!(LaunchItem::new("/usr/bin/obs").name, "obs");
    }
}
