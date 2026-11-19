//! Everything SC All-In-One does, without any user interface, so the app and
//! the command-line tool behave the same and every part can be tested
//! against real Star Citizen files.
//!
//! This project is not affiliated with Cloud Imperium Games or Roberts Space
//! Industries. It only edits the settings files the game's own options menus
//! write.

pub mod actionmaps;
pub mod attributes;
pub mod backup;
pub mod data;
pub mod device;
pub mod fsutil;
pub mod game;
pub mod install;
pub mod joysticks;
pub mod keyemu;
pub mod keymap;
pub mod keys;
pub mod launch;
pub mod mapper;
pub mod ops;
pub mod profiles;

/// Local time as shown to people and stored in files.
pub fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}
