# ShotCraft on Windows and Linux

A port of [DailyXplorer/ShotCraft](https://github.com/DailyXplorer/ShotCraft) (macOS) to Windows and Linux, with the
same features.

The macOS build runs the author's code. What Windows and Linux need sits beside it, behind
`#[cfg(not(target_os = "macos"))]`; of the author's lines, only some `#[cfg(target_os = "macos")]` gates went, where
a feature now works everywhere (scrolling capture, the capture thread), and the non-macOS placeholders the ports
replace. Everything added is Rust. System features go through Rust bindings (the `windows` crate on
Windows, pure-Rust X11 and D-Bus crates on Linux), as the Mac build does with `objc2`.

## What replaces what

| On macOS | On Windows | On Linux |
|---|---|---|
| `screencapture` (full screen, delayed, repeat area) | `xcap` (GDI) | X11: `x11rb` GetImage per RandR monitor. Wayland: the desktop's screenshot portal (`ashpd`), a plain screenshot of the desktop |
| `screencapture -i` crosshair (area, window, Space to switch) | `src/picker.rs`: a borderless full-screen window per screen over the frozen picture, with a magnifier, size readout, Space and Esc | Same on X11. Wayland: Area and Repeat Area use the same picker on a plain portal screenshot (a picker window per monitor when XWayland's monitors tile it); Window and Active Window use the portal's own picker, as apps can't learn where windows are |
| Window capture | the window's own pixels (`PrintWindow` through xcap), even when covered | what the frozen screen shows inside the window's frame |
| Vision text recognition | `Windows.Media.Ocr`, every language with its OCR feature installed | `ocrs` with built-in models (Latin script) |
| Vision barcodes | `rxing` (QR, Aztec, Data Matrix, PDF417, 1D codes) | `rxing` |
| Menu bar icon | tray icon (`tray-icon`) | StatusNotifierItem tray (`ksni`) |
| App menu bar | the same menus in a menu bar across the top of the editor (ShotCraft, Capture, File, Edit, Draw, Zoom, Help) | same |
| ⌘ shortcuts | Ctrl (shown as Ctrl+…) | Ctrl |
| Global hotkeys (`global-hotkey`) | same crate | X11: same crate. Wayland: the GlobalShortcuts portal (`portal_hotkeys.rs`; GNOME asks once for each changed set of keys, and the shortcut id carries the key) |
| `osascript` notification | Windows toast (`notify-rust`) | desktop notification over D-Bus (`notify-rust`) |
| LaunchAgent login item | `HKCU\…\Run` value | `~/.config/autostart/shotcraft.desktop` |
| `shotcraft://` links: the bundle declares the scheme, links arrive as Apple events | registered under `HKCU\Software\Classes\shotcraft` while Preferences ▸ Advanced ▸ Automation allows links (removed when switched off); the link is the launch's one argument, and a second launch hands it to the running copy | a desktop entry `~/.local/share/applications/shotcraft-links.desktop` for `x-scheme-handler/shotcraft`, made the handler with `xdg-mime`, same switch |
| Mouse pointer in screenshots (`screencapture -C`) | the cursor drawn in by GDI (`DrawIconEx` on black and on white gives its alpha, and the pixels that invert, like the I-beam's) | X11: XFixes `GetCursorImage`. Wayland: not available (the portal takes the screenshot) |
| Scrolling capture: `screencapture -i` for the area, `screencapture -R` per look, wheel events (Accessibility permission) | the area drawn on the frozen screens (`picker`, `for_scrolling`), `xcap` `capture_region` per look, `SendInput` wheel events at 120 per ~100 points (no permission needed); Esc is a global hotkey while it runs | X11: same picker, `GetImage` of the area, XTest wheel clicks (buttons 4/5, ~50 px each). Wayland: not available (apps can't see the screen); the menu items are greyed out with "(needs X11)" |
| Drag the image out (toolbar handle): an `NSDraggingSession` with the PNG's file URL | the shell's data object for the PNG (what Explorer drags) and `SHDoDragDrop`, which draws the drag picture; the temporary file stays 10 minutes (Explorer may copy it after the drop) | Wayland: `wl_data_device.start_drag` on winit's own connection (`wayland_bridge.rs`; offers `text/uri-list` and `image/png`). X11: the XDND protocol as the source (`xdnd.rs`, no pointer grab). The file is in `~/Pictures/ShotCraft drag-out`, not `/tmp`, because snap and Flatpak apps can't read `/tmp` |
| Print (`NSPrintOperation`, the system print panel) | the system print dialog (`PrintDlgW`), then GDI: the picture on one page, laid out by upstream's `print_layout` (on-screen size, shrunk to fit, centred, landscape when wider than tall), see-through parts on white | the desktop's print portal (GNOME's or KDE's dialog, Print to File included), then one page of PDF (`print_pdf.rs`) laid out by the same `print_layout`; the portal is called over D-Bus directly (`portal.rs`) because ashpd's typed answer refused GNOME's |
| Window Screenshot Background ▸ Wallpaper (the window capture comes with its shadow; the display's desktop picture behind it) | a shadow drawn like macOS's (`capture::with_shadow`: 30/30/20/46 points, blurred, falling down), then upstream's `windowbg` places it on the desktop picture (`SPI_GETDESKWALLPAPER`) as Windows' Fill lays it out; no picture → the solid colour, with a notice | the same, the picture from GNOME's settings (`gsettings`, light or dark) |
| Menu bar icon flash after a copy or a save (`menubar-done.png` for 0.6 s) | the tray icon's viewfinder with a check mark in place of the dot, for upstream's 0.6 s (`tray_flash`) | the same, on the StatusNotifierItem |
| `~/Library/Application Support/ShotCraft` | `%APPDATA%\ShotCraft` | `~/.config/ShotCraft` |
| Chalkboard / Comic Sans for hand-drawn text | Comic Sans (`C:\Windows\Fonts\comicbd.ttf`) | Comic Sans or Comic Neue if installed, else DM Sans |
| One copy per user (LaunchServices) | a second launch shows the first copy's window and quits | same |
| Hiding the editor (`orderOut`) | the editor is hidden | X11: hidden. Wayland: winit can't hide a window, so it vanishes (no frame, click-through, nothing drawn) and an error brings it back with the message in it |
| Dock icon | window and taskbar icon from `AppIcon.icns` (`ic08`) | the window's app id `shotcraft`, and a launcher entry and icon installed for the user (`launcher.rs`), since GNOME takes the icon from the entry |

## Building

```sh
cargo build --release
```

- **Windows 10/11:** nothing else needed. Text recognition uses the languages installed in Settings → Time &
  language → Language & region (each language's *Optical character recognition* feature).
- **Linux:** a desktop with a StatusNotifierItem host for the tray (KDE, most others; GNOME needs the AppIndicator
  extension) and `xdg-desktop-portal` for Wayland (screenshots, printing, hotkeys). No C libraries are linked: X11 and
  Wayland libraries are loaded at run time.

Start it hidden in the tray with `--background`, as the login item does.

## Known differences

- **Wayland:** apps can't read the screen, so captures start from a portal screenshot. Window and Active Window use
  the desktop's own picker. Scrolling capture is greyed out (a screen-recording version is possible). Pins can't stay
  on top of other windows and the thumbnail is placed by the desktop, because Wayland gives apps neither. The mouse
  pointer in screenshots is the desktop's choice. A minimized editor gets no frame callbacks, so nothing runs
  until it is restored; a hotkey or a tray click brings it back with the activation token the desktop sends, and so does
  a link when its launcher passes one (`XDG_ACTIVATION_TOKEN`). A link without a token (Firefox as a snap) shows a
  "ShotCraft is waiting" notification instead, and a click on it, which carries a token, brings the editor back. A
  stall is detected by poking the UI (a repaint request that gets no pass within 400 ms), not by the queue, because a
  link's first pass starts the capture and the result then needs another. Closing the editor with ✕ avoids all this.
- **Linux text recognition** reads Latin script only (ocrs's models). Windows reads every installed OCR language.
- **Window capture on Linux** shows what was on screen, so a covered part of the window is whatever covered it.
- **Linux checks:** the Wayland behaviour above was checked by hand on Ubuntu 26.04 (GNOME 50, Wayland). X11 is
  covered by tests under Xvfb (including drag-out, against a fake XDND target) and was not run on a real X11 desktop.
  More than one monitor is untested.
- The Mac-only option "Trim shadow" is hidden: Windows and Linux capture a window without a shadow (Wallpaper draws one).
- **Updates:** Sparkle (Check for Updates…) is macOS-only; Windows and Linux builds have no updater and never
  connect to anything.
- **Menu bar only (DECISIONS.md):** like macOS, starting ShotCraft on Windows or Linux shows the tray icon and no
  window; a window appears when you act (a capture, Open, Preferences…). Starting it again while it runs does not open
  the editor either, and the tray menu now holds every command (Current Image: Undo, Redo, Reset Crop, Rasterize, Save
  As; About; Keyboard Shortcuts), as macOS has no other menu to offer. The tray icon can be hidden only while the Show
  ShotCraft hotkey works.
- **Active Window and All Displays** (new in the macOS app) work on Windows and X11 through the same `targets.rs` the
  macOS build uses: `capture::snapshot` reads the displays (in pixels), the pointer and the focused window, and
  `targets::resolve` decides. Fullscreen and Delayed take the screen under the pointer. On Wayland the desktop's own
  screenshot dialog opens instead (it can't be asked for the active window), and All Displays is its full-desktop
  screenshot.
- The default hotkeys ⌃⌥⌘W (active window) and ⌃⌥⌘O (text recognition) are Ctrl+Shift+3 and Ctrl+Shift+4 on Windows and Linux, after
  Fullscreen (Ctrl+Shift+1) and Area (Ctrl+Shift+2). Not ⌘ → Ctrl (Ctrl+Alt+O is AltGr+O on many keyboard layouts, which types
  a letter), and not Ctrl+Alt+Win, which is awkward to press.

## Commands

Every command lives once in `src/commands.rs` (Windows and Linux): id, label, icon, shortcuts, global hotkey,
action, when it's enabled and whether it shows a check mark. The editor's menu bar, the tray menu, the editor's
keys, the toolbar's tooltips, the empty-state buttons and the Keyboard Shortcuts window are all built from it; macOS
keeps the author's menus and keys. Its tests check that ids are unique, no two commands share a shortcut, and each
shortcut finds its own command.

- Items that can't run are greyed out (Copy, Save, Pin and Zoom with no picture; Undo and Redo with nothing to undo
  or redo; Reset Crop without a crop; Rasterize with nothing drawn), and their keys do nothing.
- The menus' shortcuts work in the empty editor too (Ctrl+, Ctrl+Q Ctrl+Shift+O…), not only with a picture open.
- egui turns Ctrl+C into a Copy event and Ctrl+V into a Paste event (or nothing, when the clipboard holds a
  picture), with no key event. The table reads Copy as Ctrl+C, and runs Ctrl+V shortcuts (Open From Clipboard)
  when V comes up.
- Windows and Linux: Ctrl+C copies the image (Ctrl+Shift+C still works) and Ctrl+Y also redoes.
- Windows and Linux borrow the arrow and modifier glyphs DM Sans lacks from Segoe UI Symbol or DejaVu Sans, as macOS
  borrows them from SF.

## Credits

- [ocrs](https://github.com/robertknight/ocrs) (MIT/Apache-2.0) and its default models
  ([ocrs-models](https://github.com/robertknight/ocrs-models), CC BY-SA 4.0, trained on HierText by Google Research), in
  `assets/ocr/`, unchanged: licence text in `assets/ocr/LICENSE.txt`, attribution, sources and checksums in
  `assets/ocr/README.md` (checked against the published files on 2026-11-06). The Linux About window shows the attribution.
- [xcap](https://github.com/nashaofu/xcap) (Apache-2.0), [rxing](https://github.com/rxing-core/rxing) (Apache-2.0),
  [ksni](https://github.com/iovxw/ksni) (Unlicense), [ashpd](https://github.com/bilelmoussaoui/ashpd) (MIT),
  [x11rb](https://github.com/psychon/x11rb) (MIT/Apache-2.0), [notify-rust](https://github.com/hoodie/notify-rust)
  (MIT/Apache-2.0), [winreg](https://github.com/gentoo90/winreg-rs) (MIT), [dirs](https://github.com/dirs-dev/dirs-rs)
  (MIT/Apache-2.0), [windows](https://github.com/microsoft/windows-rs) (MIT/Apache-2.0), and on Linux
  [wayland-client, wayland-backend and wayland-protocols](https://github.com/Smithay/wayland-rs) (MIT; already in
  the tree through winit), [flate2](https://github.com/rust-lang/flate2-rs) (MIT/Apache-2.0).
- Menu icons drawn for this port, in the same 24-px outline style: `assets/icons/folder.svg`, `info.svg`,
  `timer.svg`, `window.svg`, `display.svg`, `undo.svg`, `redo.svg`, `zoom_in.svg`, `zoom_out.svg`.

## Visual tests

Pictures the app draws are compared with saved references, so a change that moves, recolours or drops
something shows up as a failing test with a picture of the difference.

- `cargo test` (always): `render::tests::exports_match_the_reference_pictures` renders a document with every
  tool, the same with a backdrop, and the four arrow styles with dark and light text, and compares them with
  `tests/reference/export/`. Pure CPU, the same on every system. Colour steps of a few levels are ignored.
- `cargo test --test ui_snapshots -- --ignored` (on request): runs the app's `--screenshot` hooks for the
  menus, Preferences tabs, About, Keyboard Shortcuts, the empty editor and the capture crosshair, and compares
  them with `tests/reference/ui-<system>/` (only Windows has references so far). It opens windows for about a
  minute and the references are for 100% display scaling; screenshot runs always use the dark theme and the
  default settings, whatever the computer uses.
- A failure leaves the new picture and a red-marked difference in `visual-diffs` (named in the message).
  After a change on purpose, `SHOTCRAFT_UPDATE_REFERENCES=1` rewrites the references; look at them first.
- Not covered: the hand-drawn text (each system's own font), the tray icon's menu (Windows draws it) and
  anything that needs real input.

## Links (`shotcraft://`)

`url_scheme.rs` (upstream) decides what a link may do, on every system. `src/links.rs` brings the links in on Windows and
Linux: the system starts ShotCraft with the link as its argument, and `links::split_args` keeps only the link (Windows
builds the command line from `"%1"`, so a link holding a quote could otherwise add `--screenshot` or a file). A copy
that is already running gets it over the single-instance port (`shotcraft:link <url>`), so a link never starts a second
copy. With no installer to declare the scheme, ShotCraft registers it itself, for the current user only, and only while
links are allowed; switching them off removes it (a registration another program made is left alone).

## Wayland

Wayland lets an app do little by itself, so ShotCraft asks the desktop. Screenshots, printing and the global hotkeys
go through the portals (`capture.rs`, `portal.rs`, `portal_hotkeys.rs`). What winit 0.30 doesn't offer
(bringing the editor back from minimized with xdg-activation, and starting a drag) is done by `wayland_bridge.rs`,
which attaches to winit's own `wl_display` (raw handles from eframe), keeps its objects on an event queue of its own
and dispatches them on one thread that wakes every 50 ms, because winit reads the same socket and may queue our
events before our wait starts. All of it is Linux-only and does nothing on X11, Windows or macOS.

## Scrolling capture

The loop is upstream's (`scroll::run_with` behind the `Page` trait: grab, scroll, wait, stitch), the same on every
system. Windows and Linux supply the rest: the area is drawn with the crosshair on the frozen screens, then those windows
close (a 300 ms wait) before the first look, so it sees the page and not them. Looks are taken in pixels and the
picture keeps the scale of its screen, as a Retina capture does on macOS. The pointer is parked in the area for the
wheel events and put back at the end. Esc, taken from every app while the capture runs, finishes it and keeps what
was stitched.
