# ShotCraft

A simple screenshot tool for macOS: capture an area, a window or the full screen, then annotate, blur, measure, read text and pin the result. Everything stays on your Mac: no account, no upload, no telemetry.

## Build

You need [Rust](https://rustup.rs) on macOS 12 or later.

```sh
./scripts/make-app.sh            # builds target/ShotCraft.app
./scripts/make-app.sh --install  # also copies it to /Applications
```

The app is signed ad hoc, and macOS asks for the Screen Recording permission on the first capture.

## Credits

* [DM Sans](https://github.com/googlefonts/dm-fonts), SIL Open Font License: `assets/fonts/OFL.txt`
* [Hugeicons](https://hugeicons.com) free icons, MIT License: `assets/icons/LICENSE-hugeicons.md`

## License

ShotCraft is released under the [MIT License](LICENSE). The fonts and icons listed in Credits keep their own licenses.
