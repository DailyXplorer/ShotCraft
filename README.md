# ShotCraft

A simple, private screenshot tool for macOS. Capture an area, a window or the full screen, then annotate, blur, measure, read text (OCR) and pin the result on screen. Everything stays on your Mac: no account, no upload, no telemetry. The only network access is the update check in release builds, which the app asks about first.

ShotCraft is written in Rust. It is an early version (0.1.0).

## Features

### Capture

* Area, any window, repeat the last area, and full-screen captures. Fullscreen captures the display the mouse pointer is on, and All Displays puts every display in one picture laid out like your desk.
* Active Window captures the frontmost window of the app you are using, without clicking. Use it from a hotkey, or from the menu bar icon menu, which targets the app that was in front before you opened it. When that app has no capturable window, you pick a window instead.
* A 3-second delayed capture of the display the mouse was on when you started it.
* Window captures can keep a transparent background or use a solid color, and can trim the window shadow.
* Instant text and QR recognition: select an area and the recognized text goes straight to the clipboard, with no editor.
* Open an existing image from a file or from the clipboard. The file picker and the command line open PNG, JPEG, GIF, BMP, TIFF and WebP files; saving writes PNG or JPEG. Animated GIFs open as their first frame.
* Runs from the menu bar. The capture commands are in the menu bar icon menu, and global hotkeys work while the editor is hidden.

### After a capture

Choose in Preferences what happens after a screenshot: open the editor, show a preview thumbnail, or do nothing. Independently of that, the screenshot can be copied to the clipboard and/or saved to your screenshots folder automatically.

The preview thumbnail sits in the bottom-right corner of the screen. Hover it for Edit, Copy, Save, Recognize Text/QR and Pin to Screen, or double-click it to edit. By default it closes by itself after 6 seconds unless the pointer is over it; Preferences can switch it to close only with its close button.

### Editor

* Annotation tools:
  * Arrow (shape, hand-drawn, line or double)
  * Text (Sans or Goofy font)
  * Rectangle and Oval (hand-drawn, transparent, translucent or opaque fill)
  * Line, Freehand drawing and Highlighter
  * Counter (numbered markers)
  * Magnifier
  * Spotlight (dims everything outside the selected areas)
  * Backdrop (gradient background with padding, shadow, inset, corner radii and aspect ratios)
* Blur tool with four modes: Blur, Blur Text, Erase and Erase Text. The text modes detect text with the macOS Vision framework and cover only those areas.
* Ruler: measure sizes in pixels or points (toggle with `P`). Hold `2` (or left/right arrows) for the horizontal span of similar pixels under the cursor, `1` (or up/down arrows) for the vertical span. You can also add rulers around a selected area.
* Color helper: press `Tab` over a pixel to copy its color, `Shift+Tab` to copy the darkest color within 20 px, or `C` with a selection to copy its average color. Eight formats are available: HEX with or without `#`, RGB (0-255 or 0-1), UIColor (Swift), CSS `rgba()`, C# Color and Android.
* Crop, Cut Out (removes a horizontal or vertical band and joins the rest), and Resize (25%, 33%, 50%, 1x, 2x, 4x, or an exact width and height). Resize cannot be undone.
* Text recognition (OCR) on a selected area, using the macOS Vision framework. It also reads QR codes and other barcodes. The result is copied to the clipboard, optionally without line breaks.
* Eight preset colors per tool plus a custom color picker, ten sizes, solid or dashed strokes.
* Undo and redo, move selected objects with the arrow keys (Shift for 10 px steps), Reset Crop, and Rasterize (merge annotations into the image).
* Zoom with the menu, shortcuts or mouse wheel (the wheel direction can be reversed), and Space + drag to pan.

### Export

* Copy the result to the clipboard as an image.
* Save to the screenshots folder (default: Desktop) as `ShotCraft-<date>-<time>.png`, or use Save Image As to pick a name and place. The format is PNG or JPEG. In Preferences, "Auto (PNG/JPEG)" saves photo-like images as JPEG (quality 90) and flat screenshots as PNG.
* Optionally downscale Retina screenshots to 1x when saving.
* Pin the result to the screen.

### Pins

A pinned image is a borderless window that stays above other windows. Drag it anywhere. Right-click for Edit, Copy, 50% / 100% / 200% size, Semi-transparent and Close; double-click to reopen it in the editor; press `Esc` to close it. You can have several pins at once.

### Preferences

* General: what happens after a screenshot (editor, thumbnail or nothing; copy; save), thumbnail auto-close, launch at startup, screenshots folder, save format, Retina downscale, window screenshot background and shadow.
* Hotkeys: record or clear a shortcut for each of the eight actions listed under Keyboard shortcuts. ShotCraft flags a shortcut that macOS refuses because another app uses it.
* Advanced: OCR line breaks, hide the menu bar icon, keep the main window on top, zoom direction, default zoom level, what Esc does when hiding the editor (copy and/or save), and confirmation style (custom notification, system notification or none).

Settings are stored in `~/Library/Application Support/ShotCraft/settings.json`.

## Keyboard shortcuts

Global hotkeys (editable in Preferences, Hotkeys):

| Action | Default |
| --- | --- |
| Fullscreen screenshot (display under the mouse) | `Cmd+Shift+1` |
| All displays screenshot | not set |
| Area screenshot | `Cmd+Shift+2` |
| Any window screenshot | not set |
| Active window screenshot | `Ctrl+Opt+Cmd+W` |
| Repeat area screenshot | not set |
| Instant Text/QR Recognition | `Ctrl+Opt+Cmd+O` |
| Show ShotCraft | not set |

In the editor:

| Keys | Action |
| --- | --- |
| `A` `T` `R` `O` `L` | Arrow, Text, Rectangle, Oval, Line |
| `D` `H` `B` `S` `C` `M` `K` | Freehand, Highlighter, Blur, Spotlight, Counter, Magnifier, Backdrop |
| `Cmd+O` | Recognize text or QR in a selected area |
| `Cmd+C`, `Cmd+S`, `Cmd+Shift+S`, `Cmd+P` | Copy, Save, Save As, Pin to Screen |
| `Cmd+Z`, `Cmd+Shift+Z` | Undo, Redo |
| `Cmd++`, `Cmd+-`, `Cmd+0`, `Cmd+1` | Zoom in, zoom out, 100%, fit |
| `Cmd+W` | Hide the editor |
| `Del` | Delete the selected object, or erase the selected area |
| `Esc` | Deselect, then copy and hide (copy on Esc is on by default) |
| `Space` + drag | Move around the picture |

Help > Keyboard Shortcuts lists them in the app.

## Privacy

ShotCraft makes no network connections of its own: the source contains no networking code and no HTTP client is among its dependencies. The one exception is the update check that [Sparkle](https://sparkle-project.org) runs in release builds (see Updates), which the app asks about first and which `make-app.sh` builds do not include. There is no account, no upload and no telemetry. Captures are taken with the system `screencapture` tool, text recognition runs through the macOS Vision framework, and images stay in memory or in the folder you choose. Temporary capture files live in the system temporary folder and are deleted after loading. Besides your screenshots, ShotCraft writes only its settings file and, if you turn on Launch at startup, a login item at `~/Library/LaunchAgents/app.shotcraft.launcher.plist`.

## Requirements

* macOS 12 or later (`LSMinimumSystemVersion` in `packaging/Info.plist`).
* The [Rust](https://rustup.rs) toolchain to build from source.

## Install

Download `ShotCraft-0.1.0.zip` from the [Releases page](https://github.com/DailyXplorer/ShotCraft/releases/latest), unzip it and move `ShotCraft.app` to `/Applications`. The app is universal (Apple Silicon and Intel).

ShotCraft is signed with its own certificate, not an Apple Developer ID, and it is not notarized, so macOS refuses to open it the first time (`spctl` reports "rejected"). To open it anyway:

* On macOS 15 or later: try to open the app once, then go to System Settings > Privacy & Security, scroll to the message about ShotCraft and click Open Anyway.
* On macOS 12 to 14: Control-click (right-click) the app, choose Open, then confirm.

You only need to do this the first time.

## Build from source

```sh
./scripts/make-app.sh            # builds target/ShotCraft.app
./scripts/make-app.sh --install  # also copies it to /Applications
```

The script runs `cargo build --release`, assembles the app bundle and signs it ad hoc (no developer certificate).

## Permissions

macOS asks for the Screen Recording permission on the first capture. If a capture fails, ShotCraft shows a message pointing to System Settings > Privacy & Security > Screen Recording. The code requests no other permission.

## Status

This is an early version. Feedback and issues are welcome on the [issue tracker](https://github.com/DailyXplorer/ShotCraft/issues).

## Updates

Releases are signed with a ShotCraft certificate rather than an Apple Developer ID, so macOS warns about the app the first time you open it (see Install for how to open it).

Updates come through [Sparkle](https://sparkle-project.org), which installs a new version only if its Ed25519 signature matches the key inside the app. On the second launch the app asks whether to check automatically. A check tells GitHub your IP address and the app version, and sends nothing else. Change your choice in the ShotCraft menu, or use Check for Updates at any time. Builds from `make-app.sh` do not include Sparkle.

## Releasing

Set the version in `Cargo.toml`, then push a tag such as `v0.1.0`. The Release workflow tests, signs and publishes the app. The tag must match the version.

## Credits

* [DM Sans](https://github.com/googlefonts/dm-fonts), SIL Open Font License: `assets/fonts/OFL.txt`
* App icon: the default [eframe](https://github.com/emilk/egui) icon, MIT or Apache-2.0 License
* [Sparkle](https://sparkle-project.org), MIT License, included in releases
* [Hugeicons](https://hugeicons.com) free icons, MIT License: `assets/icons/LICENSE-hugeicons.md`

## License

ShotCraft is released under the [MIT License](LICENSE). The fonts and icons listed in Credits keep their own licenses.
