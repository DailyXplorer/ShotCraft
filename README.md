# ShotCraft

A simple, private screenshot tool for macOS. Capture an area, a window or the full screen, then annotate, blur, measure, read text (OCR) and pin the result on screen. Everything stays on your Mac: no account, no upload, no telemetry. The only network access is the update check in release builds, which the app asks about first.

ShotCraft is written in Rust. It is an early version (0.1.2).

## Features

### Capture

* Area, any window, repeat the last area, and full-screen captures. Fullscreen captures the display the mouse pointer is on, and All Displays puts every display in one picture laid out like your desk.
* Active Window captures the frontmost window of the app you are using, without clicking. Use it from a hotkey, or from the menu bar icon menu, which targets the app that was in front before you opened it. When that app has no capturable window, you pick a window instead.
* A delayed capture (3 seconds by default, 5 or 10 in Preferences) of the display the mouse was on when you started it. The menu entry tells the wait. Preferences also has an option to put the mouse pointer in Fullscreen, Active Window, Delayed and Repeat Area screenshots; screenshots where you select an area or a window by hand never include it.
* Scrolling capture: pick an area and ShotCraft scrolls it for you, grabs it again and again and stitches what the area showed into one tall image that opens in the editor. A small dark control below the area (above or beside it when there is no room; it never covers the area and never takes the keyboard from the page) shows the live height and frame count, with Done (`Return`) and Cancel (`Esc`). Scrolling for you needs the Accessibility permission, which macOS asks for the first time; without it you scroll by hand and the capture ends by itself 3 seconds after the page stops moving. Fixed headers, footers and side panels are kept once, scrolling back up adds nothing, and the image stops at 20,000 px (Preferences changes this, and the speed). Start it from the menu bar icon or with its hotkey (not set by default); pressing the hotkey again acts as Done. Scrolling Capture (Up) is for pages that grow upwards, like a chat: scroll to the bottom of the page first, select the area, and ShotCraft scrolls up and stitches upwards.
* Active Window and Any Window captures have four looks (Preferences → General → Window Screenshot Background): **Transparent** (the default: the window with its shadow, rounded corners and see-through background), **Trim shadow** (cut tight without the shadow, corners stay rounded), **Solid Color** and **Wallpaper** (the window and its shadow laid on a color, or on your desktop picture, as an opaque rectangle; the padding is the shadow's own margin, so the window sits in the middle). If the wallpaper can't be read, Wallpaper falls back to the solid color. Area, Fullscreen and All Displays captures are never changed.
* Instant text and QR recognition: select an area and the recognized text goes straight to the clipboard, with no editor.
* Open an existing image from a file or from the clipboard. The file picker and the command line open PNG, JPEG, GIF, BMP, TIFF and WebP files; saving writes PNG or JPEG. Animated GIFs open as their first frame.
* Lives in the menu bar only: no Dock icon, and starting the app (Finder, Launch at startup, `open`) shows no window. Windows appear when you capture, open a file, open Preferences, pin an image or check for updates. Every command, including Preferences, Check for Updates, Check for Updates Automatically, About and Quit, is in the menu bar icon menu. The app has no menu bar of its own and no Cmd+Tab entry; global hotkeys work at any time.

Scrolling capture limits:

* Automatic scrolling moves the mouse pointer into the area and sends wheel events, so do not touch the mouse or keyboard until it stops. It stops at the end of the page, at the height limit, on Done or `Return` (keep the result), or on `Esc` (discard it; in manual mode with something captured, press it twice). Some apps ignore synthetic wheel events (terminals, some sandboxed windows); then the control says the page did not move and you scroll by hand. The permission has to be granted again after an update if macOS forgets the app's signature.
* By hand, scroll steadily: if the page jumps further than about 90% of the area between two grabs, or the content is blank or strongly repetitive, that step is dropped and the control says "Scroll a bit slower".
* Content that animates inside the area (video, ads, a blinking cursor beside the text) can confuse the match; small changes are tolerated.
* The image only grows in the direction of the capture (down, or up for Scrolling Capture (Up)): content on the other side of the point where you started is not added (the control says so), and scrolling back over what is already captured changes nothing. A blank band at the very top of the page is treated like a fixed header, which is harmless. The control window needs room next to the area on the same display. Esc and Return are taken from other apps while a capture runs (Return only ends the capture, it never starts one), and given back when it ends; if another ShotCraft hotkey already uses Esc, use the Cancel button.
* If the screen can't be captured for a moment (screen locked, permission prompt), the capture ends with what was stitched so far.
* Very tall images are shown in the editor as several textures, so they open at the width of the window; export and copy use the full-resolution image.

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
* Ruler: measure sizes in pixels or points (toggle with `P`). Hold `2` (or left/right arrows) for the horizontal span of similar pixels under the cursor, `1` (or up/down arrows) for the vertical span. The arrows measure only when nothing is selected (with a selection they move it) and the digits are ruler keys except with the Spotlight tool, where they set the darkness. You can also add rulers around a selected area.
* Color helper: press `Tab` over a pixel to copy its color, `Shift+Tab` to copy the darkest color within 20 px, or `C` with a selection to copy its average color. Nine formats are available: HEX with or without `#`, RGB (0-255 or 0-1), UIColor (Swift), CSS `rgba()`, C# Color, Android and OKLCH.
* Crop, Cut Out (removes a horizontal or vertical band and joins the rest), and Resize (25%, 33%, 50%, 1x, 2x, 4x, or an exact width and height). Resize cannot be undone.
* Text recognition (OCR) on a selected area, using the macOS Vision framework. It also reads QR codes and other barcodes. The result is copied to the clipboard, optionally without line breaks.
* Eight preset colors per tool plus a custom color picker, ten sizes, solid or dashed strokes.
* Undo and redo, move selected objects with the arrow keys (Shift for 10 px steps), Reset Crop, and Rasterize (merge annotations into the image).
* Add Capture (Draw menu) grabs another area and puts it beside the picture on a wider canvas.
* Zoom with the menu, shortcuts or mouse wheel (the wheel direction can be reversed), and Space + drag to pan.

### Export

* Copy the result to the clipboard as an image.
* Save to the screenshots folder (default: Desktop) as `ShotCraft-<date>-<time>.png` (the name is a template in Preferences), or use Save Image As to pick a name and place. The format is PNG or JPEG. In Preferences, "Auto (PNG/JPEG)" saves photo-like images as JPEG (quality 90) and flat screenshots as PNG.
* Optionally downscale Retina screenshots to 1x when saving.
* Pin the result to the screen.

### Pins

A pinned image is a borderless window that stays above other windows. Drag it anywhere. Right-click for Edit, Copy, 50% / 100% / 200% size, Semi-transparent and Close; double-click to reopen it in the editor; press `Esc` to close it. You can have several pins at once. A pin never grows past 90% of the screen: a picture that is too wide is scaled down, and one that is too tall (a scrolling capture) scrolls with the mouse wheel inside the pin.

### Preferences

* General: what happens after a screenshot (editor, thumbnail or nothing; copy; save), thumbnail auto-close (click outside, or the close button), launch at startup, screenshots folder, file name, save format, Retina downscale, window screenshot background (Wallpaper, Transparent, Solid Color or Trim shadow), scrolling screenshot max height (1,000 to 400,000 px, default 20,000) and scrolling screenshot speed (five levels: the slower, the shorter the steps and the longer the wait for the page to settle).
* File name: a template with `%Y` year, `%y` two-digit year, `%m` month, `%d` day, `%H` hour, `%M` minute, `%S` second, `%t` time (`%H%M%S`) and `%%`. The default is `ShotCraft-%Y%m%d-%t`. `/`, `\` and `:` become `-`. A name that exists gets `-2`, `-3`... so a save never replaces a file. The example under the field shows the result.
* Hotkeys: record or clear a shortcut for each of the nine actions listed under Keyboard shortcuts, in this order: Fullscreen, All displays, Area, Active window, Scrolling, Text/QR recognition, Repeat area, Any window, Show ShotCraft. ShotCraft flags a shortcut that macOS refuses because another app uses it. A short help explains how to free the system's own ⌘⇧3 and ⌘⇧4, with an Open System Settings button.
* Advanced: primary OCR language (Automatic, or any language Vision reads), OCR line breaks, hide the menu bar icon (only while a Show ShotCraft hotkey is set and registered, since the icon is the way back into the app), keep the main window on top, zoom direction, scrolling capture (reverse the scroll direction for apps that turn the wheel around; scroll by hand), default zoom level, default tool, what Esc does when hiding the editor (copy and/or save), confirmation style (custom notification, system notification or none), and Reset All Settings (asks first).
* Each annotation tool keeps its last colour, size and options between launches, and the default tool is the one a new screenshot opens with.

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
| Scrolling screenshot | not set |

In the editor:

| Keys | Action |
| --- | --- |
| `A` `T` `R` `O` `L` | Arrow, Text, Rectangle, Oval, Line |
| `D` `H` `B` `S` `C` `M` `K` | Freehand, Highlighter, Blur, Spotlight, Counter, Magnifier, Backdrop |
| `Cmd+O` | Recognize text or QR in a selected area |
| `Cmd+C` or `Cmd+Shift+C`, `Cmd+S`, `Cmd+Shift+S`, `Cmd+P` | Copy the image, Save, Save As, Pin to Screen |
| `Cmd+Shift+P` | Print |
| `Cmd+Z`, `Cmd+Shift+Z` | Undo, Redo |
| `Cmd++`, `Cmd+-`, `Cmd+0`, `Cmd+1` | Zoom in, zoom out, 100%, fit |
| `Cmd+W`, `Cmd+Opt+W` | Hide the editor, Close All (the editor and every pin) |
| `Cmd+Shift+O`, `Cmd+Shift+V` | Open a file, open the image on the clipboard |
| `Cmd+V` | Paste the clipboard image over the screenshot as an object you can move, resize (corner handles) and make see-through; its options bar has Save GIF for a two-frame before/after GIF |
| `Cmd+,`, `Cmd+Q` | Preferences, Quit |
| `Del` | Delete the selected object, or erase the selected area |
| `Esc` | Deselect, then copy and hide (copy on Esc is on by default) |
| `Space` + drag | Move around the picture |
| `Z` + click, `Z` + drag | Zoom in on a spot, or on the dragged area |
| `Cmd+2`, `Q`, `W` | With a selection: zoom to it, to its top left corner, to its bottom right corner |
| `Cmd` + click | Select the flat area under the pointer (a button, a bar, a background) |
| `Shift` + drag | Make the selection (or crop frame) square; shapes and lines keep their square / 45 degree constraint |
| `Opt` + drag | While selecting, preview the selection snapped to the content inside it |
| `A` | With a selection (Select tool, no object picked): snap it to the content inside |
| `B` | With a selection: blur it |
| `Enter` | With a selection: crop to it |
| `Left` `Right` `Up` `Down` | Move the selection (or the selected object) by 1 pixel, 10 with `Shift` |
| `Cmd` + arrows | Resize the selection by 1 pixel (right and bottom edge), 10 with `Shift` |
| `[` `]` | Shrink or grow the selection by 1 pixel on every side, 10 with `Shift` |
| `Cmd+Enter` | Freehand: next stroke variability (off, light, strong). Highlighter: flat or round ends |
| `Opt+Up`, `Opt+Down` | Freehand: smoother, rougher strokes |
| `1` to `9` | Spotlight tool: how dark the surroundings are (10 to 90 percent) |

Keyboard Shortcuts in the menu bar icon menu lists them in the app. The menu's shortcuts only act on the editor while the editor is the window in front: with Preferences or a pin in front, `Cmd+W` closes that window and `Cmd+Z`, `Cmd+C` and the others go to its own fields. While a hotkey is being recorded in Preferences, every other shortcut (the menu's, and ShotCraft's own global hotkeys) is off, so any combination can be recorded; `Esc` cancels and `Del` clears.

## Privacy

ShotCraft makes no network connections of its own: the source contains no networking code and no HTTP client is among its dependencies. The one exception is the update check that [Sparkle](https://sparkle-project.org) runs in release builds (see Updates), which the app asks about first and which `make-app.sh` builds do not include. There is no account, no upload and no telemetry. Captures are taken with the system `screencapture` tool, text recognition runs through the macOS Vision framework, and images stay in memory or in the folder you choose. Temporary capture files live in the system temporary folder and are deleted after loading. Besides your screenshots, ShotCraft writes only its settings file and, if you turn on Launch at startup, a login item at `~/Library/LaunchAgents/app.shotcraft.launcher.plist`.

## Requirements

* macOS 12 or later (`LSMinimumSystemVersion` in `packaging/Info.plist`).
* The [Rust](https://rustup.rs) toolchain to build from source.

## Install

Download `ShotCraft-0.1.2.zip` from the [Releases page](https://github.com/DailyXplorer/ShotCraft/releases/latest), unzip it and move `ShotCraft.app` to `/Applications`. The app is universal (Apple Silicon and Intel).

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

The app icon, window icon and menu bar icon are drawn in `assets/logo/*.svg`. After changing them, run `node scripts/make-icons.mjs` on a Mac (it needs Playwright and `iconutil`) to rewrite `assets/AppIcon.icns`, `assets/window-icon.png` and `assets/menubar.png`.

## Permissions

macOS asks for the Screen Recording permission on the first capture. If a capture fails, ShotCraft shows a message pointing to System Settings > Privacy & Security > Screen Recording. The code requests no other permission.

## Status

This is an early version. Feedback and issues are welcome on the [issue tracker](https://github.com/DailyXplorer/ShotCraft/issues).

## Updates

Releases are signed with a ShotCraft certificate rather than an Apple Developer ID, so macOS warns about the app the first time you open it (see Install for how to open it).

Updates come through [Sparkle](https://sparkle-project.org), which installs a new version only if its Ed25519 signature matches the key inside the app. On the second launch the app asks whether to check automatically. A check tells GitHub your IP address and the app version, and sends nothing else. Change your choice with Check for Updates Automatically in the menu bar icon menu, or use Check for Updates there at any time. Builds from `make-app.sh` do not include Sparkle.

## Releasing

Set the version in `Cargo.toml`, then push a tag such as `v0.1.0`. The Release workflow tests, signs and publishes the app. The tag must match the version.

## Credits

* [DM Sans](https://github.com/googlefonts/dm-fonts), SIL Open Font License: `assets/fonts/OFL.txt`
* [Sparkle](https://sparkle-project.org), MIT License, included in releases
* [Hugeicons](https://hugeicons.com) free icons, MIT License: `assets/icons/LICENSE-hugeicons.md`

## License

ShotCraft is released under the [MIT License](LICENSE). The fonts and icons listed in Credits keep their own licenses.
