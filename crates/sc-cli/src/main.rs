//! `sc-aio-cli`: SC All-In-One from the command line.
//!
//! Handy as a one-click desktop shortcut: `sc-aio-cli fix --pause`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sc_core::attributes::Group;
use sc_core::data::{Config, DataDir};
use sc_core::joysticks::Status;
use sc_core::{game, launch, ops, profiles};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "sc-aio-cli",
    version,
    about = "Fix Star Citizen joystick numbers, save and restore settings profiles, and start your companion apps."
)]
struct Cli {
    /// Game version to work on, e.g. LIVE or PTU. Defaults to the one last
    /// picked in the app, else LIVE.
    #[arg(long, global = true)]
    channel: Option<String>,
    /// The StarCitizen folder, if it is not found automatically.
    #[arg(long, global = true)]
    install: Option<PathBuf>,
    /// Wait for Enter before closing, so a desktop shortcut's window stays
    /// open long enough to read.
    #[arg(long, global = true)]
    pause: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what was found and the state of your joysticks.
    Status,
    /// Remember the current joystick setup as the right one. Do this while
    /// your bindings work.
    Remember,
    /// Move each device's bindings to the number the game gives it now.
    Fix {
        /// Show what would change without changing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Saved profiles of your settings.
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Start every enabled "Launch with" program.
    Launch,
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// List saved profiles.
    List,
    /// Save the current settings as a profile.
    Save {
        name: String,
        /// Replace a profile with the same name.
        #[arg(long)]
        force: bool,
    },
    /// Put a profile's settings back.
    Restore {
        name: String,
        /// What to restore, comma-separated: controls, user-cfg, graphics,
        /// display, audio, head-tracking, gameplay, other.
        #[arg(long, value_delimiter = ',', default_value = "controls,user-cfg,graphics,display")]
        parts: Vec<String>,
    },
    /// Delete a profile.
    Delete { name: String },
}

fn main() {
    let cli = Cli::parse();
    let pause = cli.pause;
    let code = match run(cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e:#}");
            1
        }
    };
    if pause {
        eprintln!("\nPress Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    std::process::exit(code);
}

fn run(cli: Cli) -> Result<()> {
    let data = DataDir::locate()?;
    let config = Config::load(&data)?;
    let channel = || ops::select_channel(&config, cli.install.as_deref(), cli.channel.as_deref());

    match cli.command {
        Command::Status => status(&data, &config, &channel()?),
        Command::Remember => {
            let ch = channel()?;
            let set = ops::remember(&data, &ch)?;
            println!("Remembered {} joysticks for {}:", set.devices.len(), ch.name);
            for d in &set.devices {
                println!("  js{}  {}", d.instance, d.device.name);
            }
            Ok(())
        }
        Command::Fix { dry_run } => {
            let ch = channel()?;
            let out = ops::fix_joysticks(&data, &ch, &BTreeMap::new(), dry_run)?;
            if out.plan.is_noop() {
                println!("Nothing to fix: every device's bindings are on the number the game gives it.");
                return Ok(());
            }
            for (device, from, to) in &out.plan.moves {
                println!("  {}: bindings js{from} -> js{to}", device.name);
            }
            for (label, from, to) in &out.plan.parked {
                println!("  {label} (not connected): bindings js{from} -> js{to}, out of the way");
            }
            if out.written {
                println!("Fixed {}.", ch.name);
                if let Some(b) = out.backup {
                    println!("Backup: {}", b.display());
                }
            } else {
                println!("Dry run: nothing was changed.");
            }
            Ok(())
        }
        Command::Profile(cmd) => profile(&data, cmd, channel),
        Command::Launch => {
            let enabled = config.launch.iter().filter(|i| i.enabled).count();
            if enabled == 0 {
                println!("No programs to launch. Add some on the app's \"Launch with\" page.");
                return Ok(());
            }
            let failed = launch::launch_all(&config.launch);
            for (name, err) in &failed {
                eprintln!("  {name}: {err}");
            }
            println!("Started {} of {enabled}.", enabled - failed.len());
            Ok(())
        }
    }
}

fn status(data: &DataDir, config: &Config, ch: &sc_core::install::Channel) -> Result<()> {
    let root = ch.dir.parent().context("game version folder has no parent")?;
    let versions: Vec<String> = sc_core::install::channels(root).into_iter().map(|c| c.name).collect();
    println!("Star Citizen   {}  ({})", root.display(), versions.join(", "));
    println!("Game version   {}", ch.name);
    println!("Game           {}", if game::is_running() { "running" } else { "not running" });
    println!("Data folder    {}{}", data.root.display(), if data.portable { " (portable)" } else { "" });
    println!("Launch with    {} programs", config.launch.iter().filter(|i| i.enabled).count());
    println!();

    let state = ops::joystick_state(data, ch);
    if let Some(problem) = &state.problem {
        println!("{problem}");
        return Ok(());
    }
    let Some(set) = &state.set else {
        println!("No joystick setup remembered yet. While your bindings work, run:");
        println!("  sc-aio-cli remember");
        return Ok(());
    };
    println!("Joysticks (remembered {}):", set.saved_at);
    let width = state.rows.iter().map(|r| r.device.name.len()).max().unwrap_or(6).max(6);
    println!("  {:width$}  {:12}  {:14}  {:8}  STATUS", "DEVICE", "BINDINGS ON", "GAME USES NOW", "BINDINGS");
    let js = |n: Option<u32>| n.map_or("-".to_string(), |n| format!("js{n}"));
    for r in &state.rows {
        let status = match r.status() {
            Status::Ok => "OK",
            Status::Moved => "MOVED",
            Status::NotConnected => "not connected",
            Status::New => "new",
            Status::Unclear => "unclear",
        };
        println!(
            "  {:width$}  {:12}  {:14}  {:8}  {status}",
            r.device.name,
            js(r.bindings_on),
            js(r.now),
            r.bindings
        );
    }
    let moved = state.rows.iter().filter(|r| r.status() == Status::Moved).count();
    if moved > 0 {
        println!(
            "\n{moved} device(s) have new numbers. Run `sc-aio-cli fix` to move their bindings to them."
        );
    }
    Ok(())
}

fn profile(
    data: &DataDir,
    cmd: ProfileCommand,
    channel: impl Fn() -> Result<sc_core::install::Channel>,
) -> Result<()> {
    match cmd {
        ProfileCommand::List => {
            let all = profiles::list(data)?;
            if all.is_empty() {
                println!("No profiles yet. Save one with: sc-aio-cli profile save <name>");
            }
            for p in all {
                let mut parts = Vec::new();
                if p.has_controls() {
                    parts.push("controls")
                }
                if p.has_attributes() {
                    parts.push("settings")
                }
                if p.has_user_cfg() {
                    parts.push("user.cfg")
                }
                println!(
                    "  {:24} {}  from {}  [{}]",
                    p.meta.name,
                    p.meta.created,
                    p.meta.from_channel,
                    parts.join(", ")
                );
            }
            Ok(())
        }
        ProfileCommand::Save { name, force } => {
            let ch = channel()?;
            let p = profiles::save(data, &name, &ch, force)?;
            println!("Saved \"{}\" from {}.", p.meta.name, ch.name);
            Ok(())
        }
        ProfileCommand::Restore { name, parts } => {
            let ch = channel()?;
            let p = profiles::find(data, &name)?;
            let mut opts = ops::RestoreOptions { controls: false, user_cfg: false, groups: BTreeSet::new() };
            for part in parts.iter().map(|s| s.trim().to_ascii_lowercase()) {
                match part.as_str() {
                    "controls" => opts.controls = true,
                    "user-cfg" => opts.user_cfg = true,
                    "settings" | "all-settings" => opts.groups.extend(Group::ALL),
                    other => {
                        let g = Group::from_cli_name(other)
                            .with_context(|| format!("unknown part \"{other}\""))?;
                        opts.groups.insert(g);
                    }
                }
            }
            let out = ops::restore_profile(data, &p, &ch, &opts)?;
            println!("Restored \"{}\" into {}.", p.meta.name, ch.name);
            if let Some(plan) = &out.controls {
                println!("  Controls: restored");
                for (device, from, to) in &plan.moves {
                    println!(
                        "    {} was js{from} in the profile, is js{to} now: bindings moved",
                        device.name
                    );
                }
            }
            if let Some(stats) = &out.attributes {
                println!("  Settings: {} changed, {} added", stats.changed, stats.added);
            }
            if out.user_cfg {
                println!("  user.cfg: restored");
            }
            if let Some(b) = out.backup {
                println!("Backup: {}", b.display());
            }
            Ok(())
        }
        ProfileCommand::Delete { name } => {
            profiles::delete(data, &name)?;
            println!("Deleted \"{name}\".");
            Ok(())
        }
    }
}
