# ShotCraft on Windows and Linux

A port of [DailyXplorer/ShotCraft](https://github.com/DailyXplorer/ShotCraft) (macOS) to Windows and Linux, with the
same features.

The macOS build runs the author's code. What Windows and Linux need sits beside it, behind
`#[cfg(not(target_os = "macos"))]`; of the author's lines, only some `#[cfg(target_os = "macos")]` gates went, where
a feature now works everywhere (scrolling capture, the capture thread). Everything added is Rust.

This is the groundwork both systems share: the command table, the editor's menu bar and dialogs, the crosshair
picker, the capture and scrolling-capture glue, settings and folders, hotkeys, one copy per session. Screen capture,
the tray, text recognition, link registration, the login item and printing are placeholders (they say "not
available yet") until the Windows and Linux backends arrive.

## Building

```sh
cargo build --release
```

## Known differences

- The Mac-only option "Trim shadow" is hidden: Windows and Linux capture a window without a shadow (Wallpaper draws one).
- **Updates:** Sparkle (Check for Updates…) is macOS-only; Windows and Linux builds have no updater and never
  connect to anything.
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

- [notify-rust](https://github.com/hoodie/notify-rust) (MIT/Apache-2.0), [dirs](https://github.com/dirs-dev/dirs-rs)
  (MIT/Apache-2.0).
- Menu icons drawn for this port, in the same 24-px outline style: `assets/icons/folder.svg`, `info.svg`,
  `timer.svg`, `window.svg`, `display.svg`, `undo.svg`, `redo.svg`, `zoom_in.svg`, `zoom_out.svg`.

## Visual tests

Pictures the app draws are compared with saved references, so a change that moves, recolours or drops
something shows up as a failing test with a picture of the difference.

- `cargo test`: `render::tests::exports_match_the_reference_pictures` renders a document with every tool, the same
  with a backdrop, and the four arrow styles with dark and light text, and compares them with
  `tests/reference/export/`. Pure CPU, the same on every system. Colour steps of a few levels are ignored.
- A failure leaves the new picture and a red-marked difference in `visual-diffs` (named in the message).
  After a change on purpose, `SHOTCRAFT_UPDATE_REFERENCES=1` rewrites the references; look at them first.
- Not covered: the hand-drawn text (each system's own font) and anything that needs real input.

## Links (`shotcraft://`)

`url_scheme.rs` (upstream) decides what a link may do, on every system. `src/links.rs` brings the links in on Windows and
Linux: the system starts ShotCraft with the link as its argument, and `links::split_args` keeps only the link (Windows
builds the command line from `"%1"`, so a link holding a quote could otherwise add `--screenshot` or a file). A copy
that is already running gets it over the single-instance port (`shotcraft:link <url>`), so a link never starts a second
copy. Registering the link type is each system's part (a placeholder here).

## Scrolling capture

The loop is upstream's (`scroll::run_with` behind the `Page` trait: grab, scroll, wait, stitch), the same on every
system. Windows and Linux supply the rest: the area is drawn with the crosshair on the frozen screens, then those windows
close (a 300 ms wait) before the first look, so it sees the page and not them. Looks are taken in pixels and the
picture keeps the scale of its screen, as a Retina capture does on macOS. Esc, taken from every app while the capture
runs, finishes it and keeps what was stitched. Grabbing the screen and scrolling the page are each system's part.
