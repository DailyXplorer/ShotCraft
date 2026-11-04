#!/bin/sh
# Builds the signed update in target/dist: ShotCraft-<version>.zip and appcast.xml.
# Needs SHOTCRAFT_SIGN_IDENTITY, a code signing identity in the keychain, and SPARKLE_ED_KEY.
set -eu
cd "$(dirname "$0")/.."

sparkle_version=2.10.1
sparkle_sha256=1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27
repo=DailyXplorer/ShotCraft

: "$SHOTCRAFT_SIGN_IDENTITY" "$SPARKLE_ED_KEY"

sparkle=target/sparkle
rm -rf "$sparkle" target/dist
mkdir -p "$sparkle" target/dist
curl -fsSL "https://github.com/sparkle-project/Sparkle/releases/download/$sparkle_version/Sparkle-$sparkle_version.tar.xz" -o "$sparkle/sparkle.tar.xz"
echo "$sparkle_sha256  $sparkle/sparkle.tar.xz" | shasum -a 256 -c -
tar -xf "$sparkle/sparkle.tar.xz" -C "$sparkle"

SHOTCRAFT_UNIVERSAL=1 scripts/make-app.sh

app=target/ShotCraft.app
framework="$app/Contents/Frameworks/Sparkle.framework"
mkdir -p "$app/Contents/Frameworks"
ditto "$sparkle/Sparkle.framework" "$framework"

# Inside out, as a signature covers what it contains. No hardened runtime: with a self-signed
# identity, its library validation would refuse to load Sparkle.
sign() {
    codesign --force --timestamp=none --sign "$SHOTCRAFT_SIGN_IDENTITY" "$@"
}
sign "$framework/Versions/B/XPCServices/Installer.xpc"
sign --preserve-metadata=entitlements "$framework/Versions/B/XPCServices/Downloader.xpc"
sign "$framework/Versions/B/Autoupdate"
sign "$framework/Versions/B/Updater.app"
sign "$framework"
sign "$app"

codesign --verify --deep --strict "$app"
requirement=$(codesign --display --requirements - "$app" 2>&1)
case "$requirement" in
    *cdhash*) echo "The signature has no stable identity: $requirement" >&2; exit 1 ;;
esac
"$app/Contents/MacOS/shotcraft" --check-updater

version=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$app/Contents/Info.plist")
build=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$app/Contents/Info.plist")
minimum=$(/usr/libexec/PlistBuddy -c "Print :LSMinimumSystemVersion" "$app/Contents/Info.plist")
zip="ShotCraft-$version.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "target/dist/$zip"

attributes=$(printf '%s' "$SPARKLE_ED_KEY" | "$sparkle/bin/sign_update" --ed-key-file - "target/dist/$zip")
signature=$(echo "$attributes" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p')
printf '%s' "$SPARKLE_ED_KEY" | "$sparkle/bin/sign_update" --ed-key-file - --verify "target/dist/$zip" "$signature"

cat > target/dist/appcast.xml <<EOF
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>ShotCraft</title>
    <item>
      <title>$version</title>
      <sparkle:version>$build</sparkle:version>
      <sparkle:shortVersionString>$version</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>$minimum</sparkle:minimumSystemVersion>
      <sparkle:releaseNotesLink>https://github.com/$repo/releases/tag/v$version</sparkle:releaseNotesLink>
      <enclosure url="https://github.com/$repo/releases/download/v$version/$zip" type="application/octet-stream" $attributes/>
    </item>
  </channel>
</rss>
EOF
xmllint --noout target/dist/appcast.xml
echo "Built target/dist/$zip"
