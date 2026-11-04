#!/bin/sh
# Builds ShotCraft.app in target/. With --install, also copies it to /Applications.
# SHOTCRAFT_UNIVERSAL=1 builds for both Apple Silicon and Intel.
set -eu
cd "$(dirname "$0")/.."

if [ "${SHOTCRAFT_UNIVERSAL:-}" = 1 ]; then
    cargo build --release --target aarch64-apple-darwin --target x86_64-apple-darwin
    mkdir -p target/release
    lipo -create target/aarch64-apple-darwin/release/shotcraft target/x86_64-apple-darwin/release/shotcraft -output target/release/shotcraft
else
    cargo build --release
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
build=$(echo "$version" | awk -F. '{ print $1 * 1000000 + $2 * 1000 + $3 }')

app=target/ShotCraft.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/shotcraft "$app/Contents/MacOS/shotcraft"
cp packaging/Info.plist "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" -c "Set :CFBundleVersion $build" "$app/Contents/Info.plist"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
codesign --force --sign - "$app"

if [ "${1:-}" = "--install" ]; then
    rm -rf /Applications/ShotCraft.app
    ditto "$app" /Applications/ShotCraft.app
    echo "Installed /Applications/ShotCraft.app"
fi
