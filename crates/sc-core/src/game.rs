//! Is Star Citizen running?
//!
//! The game rewrites its settings files when it exits, which would undo any
//! change made while it runs, so nothing is written until it is closed. Under
//! Wine the game shows up as a normal Linux process with the same name.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const GAME_EXE: &str = "starcitizen.exe";

pub fn is_running() -> bool {
    let Ok(mut sys) = System::new() else {
        return false;
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    sys.processes().values().any(|p| {
        p.name().to_string_lossy().eq_ignore_ascii_case(GAME_EXE)
            || p.exe()
                .and_then(|e| e.file_name())
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(GAME_EXE))
    })
}

pub fn ensure_closed() -> anyhow::Result<()> {
    if is_running() {
        anyhow::bail!(
            "Star Citizen is running. Close it first: the game rewrites these files when it exits, which would undo the change."
        );
    }
    Ok(())
}
