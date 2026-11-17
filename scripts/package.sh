#!/usr/bin/env bash
# Build the portable release folder and archive it:
#
#   dist/sc-aio/             the folder people unpack and run
#   dist/sc-aio-<version>-windows-x86_64.zip   or
#   dist/sc-aio-<version>-linux-x86_64.tar.gz
#
# Run after `cargo build --release -p sc-gui -p sc-cli`. Works in Git Bash on
# Windows (as on the GitHub runner) and on Linux.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows; exe=.exe ;;
  Linux) os=linux; exe= ;;
  *) echo "unsupported system: $(uname -s)" >&2; exit 1 ;;
esac

out=dist/sc-aio
rm -rf dist
mkdir -p "$out/licenses"
cp "target/release/sc-aio$exe" "target/release/sc-aio-cli$exe" "$out/"
cp README.md "$out/README.md"
cp LICENSE "$out/LICENSE.txt"
cp crates/sc-gui/assets/fonts/OFL.txt "$out/licenses/Inter-font-OFL.txt"
if [ -f dist-licenses.html ]; then cp dist-licenses.html "$out/licenses/third-party.html"; fi

# Portable by default: with this file next to the programs, settings,
# profiles, remembered joysticks and backups all go in data/ beside them.
cat > "$out/portable.txt" <<'TXT'
While this file is here, SC All-In-One keeps everything it saves (settings,
profiles, remembered joysticks and backups) in the "data" folder next to it,
so the whole tool is this one folder: move it or copy it and it all comes
along.

Delete this file to keep that data in your user profile instead
(%APPDATA%\sc-aio on Windows, ~/.local/share/sc-aio on Linux).
TXT

if [ "$os" = windows ]; then
  archive="dist/sc-aio-$version-windows-x86_64.zip"
  if command -v 7z >/dev/null; then
    (cd dist && 7z a -tzip "../$archive" sc-aio >/dev/null)
  else
    powershell -NoProfile -Command "Compress-Archive -Path dist/sc-aio -DestinationPath $archive"
  fi
else
  archive="dist/sc-aio-$version-linux-x86_64.tar.gz"
  tar -czf "$archive" -C dist sc-aio
fi
echo "$archive"
