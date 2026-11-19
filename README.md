# SC All-In-One

A small, free tool for Star Citizen pilots, for Windows and Linux. It does three things:

- **Joysticks.** Star Citizen numbers your sticks, throttles and pedals (js1, js2, …) in the order Windows
  lists them, and that order can change after switching USB ports or replugging a device. Your bindings stay
  on the old numbers, so they end up on the wrong device. SC All-In-One remembers which device each set of
  bindings belongs to, spots when the numbers change, and moves the bindings back to the right devices in one
  click.
- **Profiles.** Save your settings under a name and restore them after a patch, a reinstall, or onto another
  game version (LIVE, PTU, …). A profile holds your bindings with each device's saturation, deadzones,
  curves and inverts; your graphics, display and other settings; and `user.cfg`. Restore only the parts you
  want: graphics without audio, say. Bindings are moved to whatever numbers your devices have today.
- **Launch with.** Start TrackIR, JoyToKey, OBS, the RSI Launcher or anything else together, with one click.

It works with any device the game sees: VKB, Virpil, Thrustmaster, Winwing, Logitech and the rest.

> Not affiliated with Cloud Imperium Games or Roberts Space Industries. It only edits the settings files that
> the game's own options menus write.

## Download and run

Nothing to install. Download the latest release, unpack it anywhere, and run it:

- **Windows:** `sc-aio-<version>-windows-x86_64.zip`, then run `sc-aio.exe`.
- **Linux:** `sc-aio-<version>-linux-x86_64.tar.gz`, then run `./sc-aio`. Works as-is on Bazzite and other
  immutable distros; nothing is installed. An AppImage is planned.

Everything the tool saves (settings, profiles, remembered joysticks and backups) goes in a `data` folder
next to it, so the whole tool is one folder you can move or copy. Delete `portable.txt` to keep that data in
your user profile instead.

Windows may warn about an unrecognised publisher, because the build is not code-signed. Choose *More info*,
then *Run anyway*.

## First steps

1. **Star Citizen is found automatically** in the usual places: on Windows, `Roberts Space Industries\StarCitizen`
   on any drive; on Linux, Wine prefixes such as `~/Games/star-citizen` (LUG helper, Lutris) and Bottles. If it
   is somewhere else, add the folder under **Settings**.
2. On **Joysticks**, press **Remember current setup** while your bindings work.
3. On **Profiles**, save your current settings.

From then on, if a device's number changes, the Joysticks page says so and **Fix joysticks** moves the
bindings. Close the game first: it rewrites these files when it exits.

## How the joystick fix works

The game decides the numbers, so the tool cannot change them. Instead it moves each device's bindings to the
number the device has now.

To know whose bindings are whose, it compares contents rather than trusting numbers: when you remember your
setup, it notes which actions each device has bound, and later finds those bindings wherever they are in the
game's file. The number each device has *now* comes from the game's file too. If that looks wrong, you can
set it by hand on the Joysticks page; to check, bind any button in the game's keybinding screen and see which
`jsN` it shows.

Devices that are not connected keep their bindings, moved out of the way if another device now has their
number. Two identical devices with the same name and hardware ID cannot be told apart and are left alone; give
one of them its own ID in its maker's configuration tool (VKBDevCfg, VPC Configuration Tool, …).

Every change is checked against the game running and backed up first. The newest 40 backups are kept in
`data/backups`.

## Command line

`sc-aio-cli` does the same from a terminal or a desktop shortcut:

```
sc-aio-cli status                        what was found and how your joysticks look
sc-aio-cli remember                      remember the current joystick setup
sc-aio-cli fix [--dry-run]               move bindings to the devices' current numbers
sc-aio-cli profile list
sc-aio-cli profile save "Gunfighters"
sc-aio-cli profile restore "Gunfighters" --parts controls,graphics,display
sc-aio-cli launch                        start every enabled "Launch with" program
```

Add `--channel PTU` to work on another game version, `--install <folder>` if the game is not found, and
`--pause` to keep the window open when run from a shortcut (`sc-aio-cli fix --pause`).

## Building from source

Install [Rust](https://rustup.rs), then:

```
cargo build --release
```

The programs are `target/release/sc-aio` (the app) and `target/release/sc-aio-cli`. On Linux no extra
system packages are needed to build. `cargo test` runs the tests, which use real Star Citizen files from
`crates/sc-core/tests/fixtures`.

The code is in three parts: `sc-core` (everything the tool does, with no user interface), `sc-cli` and
`sc-gui` (the app, built with [egui](https://github.com/emilk/egui)).

## Licence

Free software under the GNU General Public License, version 3 or later: see [LICENSE](LICENSE). The app
includes the [Inter](https://rsms.me/inter/) font under the SIL Open Font License; release downloads list
the licences of every library they are built from in `licenses/`.
