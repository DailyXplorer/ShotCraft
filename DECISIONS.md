# Product decisions

Rules that shape the app. Change one only on purpose, and say so in the pull request.

## Menu bar only: nothing opens at launch

Starting ShotCraft (Finder, `open`, a login item) shows the menu bar icon and no window. A window appears only when the user acts: a capture, opening a file, Preferences, a pin, an update dialog. Starting with an image file as argument opens it in the editor. Opening the app again while it runs does not open the editor.

Why: a capture tool runs all day and must not take over the screen at login.

## No Dock icon

`LSUIElement` is true in `packaging/Info.plist`, and the app sets the accessory activation policy before it creates a window, so a build run outside a bundle behaves the same. The consequences are part of the rule:

* There is no main menu bar. Every command that used to live only there is in the menu bar icon menu.
* Every window that opens activates the app and comes to the front, since the system does not do it for an accessory app. This includes Sparkle's dialogs.
* The main menu is still installed, because its key equivalents (Cmd+, Cmd+P, Cmd+W, Cmd+Q) keep working while a ShotCraft window has focus even though the menu is not drawn. The other editor shortcuts (Cmd+C, Cmd+S, Cmd+Z, ...) are handled by the editor itself. Check a new shortcut with the Dock icon off.
* The app has no Cmd+Tab entry. This is accepted.
* The menu bar icon can be hidden only while the Show ShotCraft hotkey is set and registered, so the app always stays reachable.

## URL scheme shotcraft:// added (owner approved)

ShotCraft answers `shotcraft://` links (captures, `ocr`, `load/clipboard`, `load/file`, `show`, `settings`), documented in the README under Automation. The owner approved it after an earlier version left it out.

* Only the routes in `src/url_scheme.rs` exist; anything else is ignored. No link runs a command, and the only file a link can make the app read is an image named by `load/file` (absolute path, no `..`, image extension, regular file once symlinks are resolved).
* Links are off until the user allows them (Preferences, Advanced; the first link asks). Allowing is the user's act, never a link's.
* Not offered: a `then=` parameter and `settings/...` toggles. A link starts an action; what happens after a capture, and every preference, stays in Preferences.

## No screenshot library (owner decision)

ShotCraft keeps no history or library of past screenshots. A capture is copied, saved to the screenshots folder, pinned or edited, and then it is the user's file. This is on purpose, so no link or preference for a library exists.

## Scrolling capture has no on-screen control: Esc finishes and keeps the capture (owner decision)

While a scrolling capture runs (down or up, automatic or by hand) no ShotCraft window is on screen: there is no HUD, no Done or Cancel button, and Return is not a hotkey. `Esc`, registered as a global hotkey only for the length of the capture and always given back when it ends, finishes the capture and keeps it: the result opens the normal way (after-capture action, editor). Esc with nothing captured yet ends silently. The capture also ends by itself at the end of the content, at the height limit, and in manual mode 3 seconds after the page stops moving. What a window used to say (Accessibility permission missing, scroll slower, the page did not move) is at most one macOS notification per capture, and only when the user has to act.

Why: the owner wants it to work like the best-known tools of its kind: select, let it scroll, press Esc, get the picture, with nothing drawn over the page that is being captured.

## Preferences: no helper text under rows; explanations live in tooltips (info icon) (owner decision)

No Preferences tab (General, Hotkeys, Advanced) draws grey explanatory lines under a row. A row that needs an explanation carries a small dim info icon (ⓘ) right after its label, and the text shows as a tooltip on hovering the icon or the label. The file name example and its `%` tokens, the hotkey reassignment steps, and the notes on capture, pointer, scrolling and automation rows all follow this rule.

* A message the user must act on (a refused hotkey, a missing permission, the reset confirmation) stays visible: only static help moves into tooltips.
* A new preference gets its explanation as a tooltip (`tip_row` in `src/prefs.rs`), never as a line under the row.

Why: the lines crowded the window; the owner wants a plain page with help one hover away.

## Windows and Linux (proposed, for the owner's approval)

The rules above hold on every system. These entries say how Windows and Linux follow them where macOS has something they lack. Each one can be accepted or turned down on its own.

### The tray icon is the menu bar icon, with a one-time tip on Windows

On Windows and Linux the tray (notification area) icon does what the menu bar icon does on macOS: same menu, same rule that it can be hidden only while the Show ShotCraft hotkey works. Windows 11 puts a new tray icon in the hidden overflow, so on the first start, and only if the icon is not already on the taskbar, ShotCraft shows one notification saying where the icon is and how to pin it. It never shows again.

Why: without the tip a first-time user sees nothing at all after starting the app, because of "Menu bar only".

### The editor has its own menu bar on Windows and Linux

macOS keeps no main menu bar ("No Dock icon"). Windows and Linux have no system menu bar to fall back on, so the editor window draws one across its top with the same menus and items as the macOS main menu. Its shortcuts use Ctrl where macOS uses Cmd.

Why: on those systems a window's commands are expected in the window; without it they would exist only as shortcuts.

### Default hotkeys are Ctrl+Shift+1 to 4

Full screen, Area, Active window and Text recognition default to Ctrl+Shift+1, 2, 3 and 4 on Windows and Linux. Not Ctrl+Alt+letter: on many keyboard layouts Ctrl+Alt is AltGr, and AltGr+letter types a character there.

### shotcraft:// is registered only while links are allowed

There is no installer on Windows and Linux to declare the URL scheme, so ShotCraft registers it itself for the current user (Windows: `HKCU\Software\Classes\shotcraft`; Linux: a desktop entry and `xdg-mime`) when the user allows links in Preferences, and removes it when they turn links off. Everything in "URL scheme shotcraft:// added" still applies.

Why: an app without an installer should leave nothing behind in the system that the user did not turn on. The difference from macOS: a link can start ShotCraft only after links are allowed, so the "first link asks" question does not come up there.

### The mouse wheel scrolls the picture; Ctrl+wheel zooms

On Windows and Linux the wheel scrolls a picture that is larger than the window, Shift+wheel scrolls sideways and Ctrl+wheel zooms (Reverse zoom applies). The canvas shows scroll bars, and Page Up, Page Down, Home and End move through a long capture. Scrolling stops at the picture's edges.

Why: a mouse on those systems usually has a wheel and no trackpad, and long scrolling captures need a way to move through them.

### One copy per user session

On Windows and Linux a second start of ShotCraft does not run a second copy. It tells the running one to show itself (or passes on the `shotcraft://` link it was started with) and quits. macOS does this for the app bundle by itself.
