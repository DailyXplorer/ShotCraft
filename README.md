# ShotCraft

A simple screenshot tool for macOS: capture an area, a window or the full screen, then annotate, blur, measure, read text and pin the result. Everything stays on your Mac: no account, no upload, no telemetry. The only network access is the update check, which the app asks about first.

## Build

You need [Rust](https://rustup.rs) on macOS 12 or later.

```sh
./scripts/make-app.sh            # builds target/ShotCraft.app
./scripts/make-app.sh --install  # also copies it to /Applications
```

The app is signed ad hoc, and macOS asks for the Screen Recording permission on the first capture.

## Updates

Releases are signed with a ShotCraft certificate rather than an Apple Developer ID, so macOS warns about the app the first time you open it. Control-click the app and choose Open.

Updates come through [Sparkle](https://sparkle-project.org), which installs a new version only if its Ed25519 signature matches the key inside the app. On the second launch the app asks whether to check automatically. A check tells GitHub your IP address and the app version, and sends nothing else. Change your choice in the ShotCraft menu, or use Check for Updates at any time. Builds from `make-app.sh` do not include Sparkle.

## Releasing

Set the version in `Cargo.toml`, then push a tag such as `v0.1.0`. The Release workflow tests, signs and publishes the app. The tag must match the version.

## Credits

* [DM Sans](https://github.com/googlefonts/dm-fonts), SIL Open Font License: `assets/fonts/OFL.txt`
* [Sparkle](https://sparkle-project.org), MIT License, included in releases
* [Hugeicons](https://hugeicons.com) free icons, MIT License: `assets/icons/LICENSE-hugeicons.md`
