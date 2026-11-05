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
