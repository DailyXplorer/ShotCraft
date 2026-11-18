# Product decisions

Rules that shape the app. Change one only on purpose, and say so in the pull request.

## Menu bar only: nothing opens at launch

Starting ShotCraft (Finder, `open`, a login item) shows the menu bar icon and no window. A window appears only when the user acts: a capture, opening a file, Preferences, a pin, an update dialog. Starting with an image file as argument opens it in the editor, and so does a file the system hands over: Finder's Open With, a file dropped on the app's icon, `open -a ShotCraft file.png`. Opening the app again while it runs does not open the editor.

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

While a scrolling capture runs (down or up, automatic or by hand) no ShotCraft window is on screen: there is no HUD, no Done or Cancel button, and Return is not a hotkey. `Esc`, registered as a global hotkey only for the length of the capture and always given back when it ends, finishes the capture and keeps it: the result opens the normal way (after-capture action, editor). Esc with nothing captured yet ends silently. The capture also ends by itself at the end of the content, at the height limit, and in manual mode 3 seconds after the page stops moving. A page that never moved at all (it cannot scroll, or nobody scrolled it) ends the capture after 10 seconds, and what the area showed opens like any other capture. This last rule is proposed, for the owner's approval: without it such a capture waited for Esc with nothing on screen to say so. What a window used to say (Accessibility permission missing, scroll slower, the page did not move) is at most one macOS notification per capture, and only when the user has to act.

Why: the owner wants it to work like the best-known tools of its kind: select, let it scroll, press Esc, get the picture, with nothing drawn over the page that is being captured.

## Preferences: no helper text under rows; explanations live in tooltips (info icon) (owner decision)

No Preferences tab (General, Hotkeys, Advanced) draws grey explanatory lines under a row. A row that needs an explanation carries a small dim info icon (ⓘ) right after its label, and the text shows as a tooltip on hovering the icon or the label. The file name example and its `%` tokens, the hotkey reassignment steps, and the notes on capture, pointer, scrolling and automation rows all follow this rule.

* A message the user must act on (a refused hotkey, a missing permission, the reset confirmation) stays visible: only static help moves into tooltips.
* A new preference gets its explanation as a tooltip (`tip_row` in `src/prefs.rs`), never as a line under the row.

Why: the lines crowded the window; the owner wants a plain page with help one hover away.

## macOS: one bar in the title bar, no status bar (proposed, for the owner's approval)

On macOS the editor has a single bar, in the title bar, laid out like the reference app's: the window buttons, then Copy, Save, Pin, the drag handle and the tools from the left, and at the right the colour, size and zoom cells. The bar has no Preferences button, as the reference app's has none: Preferences is in the menu bar icon menu and on Cmd+,. The buttons have no frame, and the selected tool sits on a grey tile. There is no status bar at the bottom. The options bar under it is unchanged.

* The bar is 52 points tall and white in the light appearance.
* A window 836 points wide, the smallest one a capture opens in, shows eight tools and the three cells. As the window narrows the tools move into the More menu, then the size cell goes. The colour and zoom cells, Copy, Save, Pin, the drag handle and More stay down to the smallest window.
* The size cell says "Image size" when nothing is selected. With a selection it says "Selection Size", or the contrast of the selection when there is one to show: the bar has no room for a contrast cell.
* A long colour format is cut short in its cell. The colour popover shows it whole.
* Windows has the same bar under its own title bar (see "Windows: the macOS bar, with a menu button", below). Linux keeps the centred tools and the status bar (`sizing::Chrome::Split`).

Why: the owner wants the Mac app to look like the reference app, where all of this lives in one bar.

## macOS: the editor window fits the picture (proposed, for the owner's approval)

On macOS a picture opens in a window of its own size, with the bar above it: a picture of 1000 × 600 points opens in a window of 1000 × 652. These are the reference app's sizes, measured on a screen of 1512 × 982 points.

* The window is at least 836 × 470 points. A smaller picture sits in the middle of it.
* The window is at most 80% of the screen's width, and the screen's free height less 112 points. A larger picture is shrunk to fit, with 9 points free around it.
* The window opens a little above the middle of the screen, where AppKit centres a window.
* The options bar is not counted. When it shows, the window grows by its 48 points at the bottom edge, and it gives them back when the bar goes, so the picture neither moves nor changes zoom. A window that has no room to grow shows the bar inside its own height.
* Windows opens the same windows. Its screen is the one without the taskbar, less the title bar and the borders Windows draws around the window. Linux keeps the wide, near-square window with a margin around the picture (`sizing::Chrome::editor_window`).

Why: the window used to be 1244 points wide for every picture, mostly empty around a small capture.

## Windows and Linux (proposed, for the owner's approval)

The rules above hold on every system. These entries say how Windows and Linux follow them where macOS has something they lack. Each one can be accepted or turned down on its own.

### The tray icon is the menu bar icon, with a one-time tip on Windows

On Windows and Linux the tray (notification area) icon does what the menu bar icon does on macOS: same menu, same rule that it can be hidden only while the Show ShotCraft hotkey works. Windows 11 puts a new tray icon in the hidden overflow, so on the first start, and only if the icon is not already on the taskbar, ShotCraft shows one notification saying where the icon is and how to pin it. It never shows again.

Why: without the tip a first-time user sees nothing at all after starting the app, because of "Menu bar only".

### The editor keeps its menus in the window on Windows and Linux

macOS keeps no main menu bar ("No Dock icon"). Windows and Linux have no system menu bar to fall back on, so the editor window holds the same menus and items as the macOS main menu. On Linux they are a menu bar across the top of the window. On Windows they are behind the menu button of the editor's bar (next entry). The shortcuts use Ctrl where macOS uses Cmd.

Why: on those systems a window's commands are expected in the window; without it they would exist only as shortcuts.

### Windows: the macOS bar, with a menu button where the window buttons are

On Windows the editor has the one bar of the macOS editor: the same tools from the left, the same colour, size and zoom cells at the right, the same height, no status bar and no Preferences button. The window opens at the size of the picture and grows by the options bar, as on macOS. Windows draws its own title bar above the bar, with the window buttons at the right, so the left end of the bar is free. A menu button sits there. It opens ShotCraft, Capture, File, Edit, Draw, Zoom and Help as submenus.

* The bar starts 30 points earlier than on macOS, so it has room for one more tool at every width.
* The title bar stays the one Windows draws. It moves, snaps, maximizes and closes the window as in any Windows app. The tools are not in it: that needs a title bar ShotCraft draws and handles itself, which nobody could try on a real PC for this change.
* Linux keeps its menu bar, its centred tools and its status bar. Nothing was changed there.

Why: the owner wants the Windows app to be like the Mac app. The menu bar above the tools and the status bar under the picture were what a Windows user saw and a Mac user did not.

### Windows: an installer, and nothing else to install

ShotCraft for Windows ships as `ShotCraft-<version>-Setup.exe`, built by `scripts/windows/make-installer.ps1` and published with each release.

* It installs for the current user, without administrator rights. Its one question is whether to add a desktop shortcut. `/ALLUSERS` on the command line installs for everyone.
* Nothing has to be installed first. `shotcraft.exe` is a native Windows program: it needs no Rust, no .NET and no Visual C++ runtime on the PC. The C runtime is linked into it, and the installer build fails if that stops being true.
* The one thing ShotCraft uses that a PC may lack is Windows' own text recognition for the user's language. When the installer finds none, it asks. On a yes, Windows installs it, which needs administrator rights and Windows Update. The installer waits five minutes for it at most, then leaves Windows to finish by itself. A no changes nothing else.
* Explorer lists ShotCraft under Open with for PNG, JPEG, GIF, BMP, TIFF and WebP pictures. ShotCraft does not become their default app.
* The installer does not turn on Launch at Startup. That stays ShotCraft's own setting, as on macOS, so the setting and what Windows does cannot disagree.
* An update is the newer installer run over the old one. It asks before it closes a running ShotCraft. An update that runs unattended starts ShotCraft again.
* Uninstalling removes the program, its Start menu entry, its Open with entry, and what ShotCraft registered by itself while it ran from there: the login item and the `shotcraft://` link type. It asks before it removes the settings, and keeps them when it runs unattended.
* The installer is not signed, so Windows SmartScreen warns about it the first time, as macOS warns about the app.

Why: the owner wants an installer that leaves a PC ready, without sending the user to fetch something else first.

### Default hotkeys are Ctrl+Shift+1 to 4

Full screen, Area, Active window and Text recognition default to Ctrl+Shift+1, 2, 3 and 4 on Windows and Linux. Not Ctrl+Alt+letter: on many keyboard layouts Ctrl+Alt is AltGr, and AltGr+letter types a character there.

### shotcraft:// is registered only while links are allowed

The Windows installer does not declare the URL scheme and Linux has no installer, so ShotCraft registers the scheme itself for the current user (Windows: `HKCU\Software\Classes\shotcraft`; Linux: a desktop entry and `xdg-mime`) when the user allows links in Preferences, and removes it when they turn links off. The Windows uninstaller removes a registration that points at the copy it removes. Everything in "URL scheme shotcraft:// added" still applies.

Why: ShotCraft should put nothing in the system that the user did not turn on. The difference from macOS: a link can start ShotCraft only after links are allowed, so the "first link asks" question does not come up there.

### The mouse wheel scrolls the picture; Ctrl+wheel zooms

On Windows and Linux the wheel scrolls a picture that is larger than the window, Shift+wheel scrolls sideways and Ctrl+wheel zooms (Reverse zoom applies). The canvas shows scroll bars, and Page Up, Page Down, Home and End move through a long capture. Scrolling stops at the picture's edges.

Why: a mouse on those systems usually has a wheel and no trackpad, and long scrolling captures need a way to move through them.

### One copy per user session

On Windows and Linux a second start of ShotCraft does not run a second copy. It tells the running one to show itself (or passes on the `shotcraft://` link or the picture it was started with) and quits. macOS does this for the app bundle by itself.
