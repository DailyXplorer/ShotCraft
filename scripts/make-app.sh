#!/bin/sh
# Builds ShotCraft.app in target/. With --install, also copies it to /Applications.
set -eu
cd "$(dirname "$0")/.."

cargo build --release

app=target/ShotCraft.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/shotcraft "$app/Contents/MacOS/shotcraft"
cp packaging/Info.plist "$app/Contents/Info.plist"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
codesign --force --sign - "$app"

if [ "${1:-}" = "--install" ]; then
    rm -rf /Applications/ShotCraft.app
    ditto "$app" /Applications/ShotCraft.app
    echo "Installed /Applications/ShotCraft.app"
fi
