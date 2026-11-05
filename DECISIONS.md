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

## Scrolling capture has no on-screen control: Esc finishes and keeps the capture (owner decision)

While a scrolling capture runs (down or up, automatic or by hand) no ShotCraft window is on screen: there is no HUD, no Done or Cancel button, and Return is not a hotkey. `Esc`, registered as a global hotkey only for the length of the capture and always given back when it ends, finishes the capture and keeps it: the result opens the normal way (after-capture action, editor). Esc with nothing captured yet ends silently. The capture also ends by itself at the end of the content, at the height limit, and in manual mode 3 seconds after the page stops moving. What a window used to say (Accessibility permission missing, scroll slower, the page did not move) is at most one macOS notification per capture, and only when the user has to act.

Why: the owner wants it to work like the best-known tools of its kind: select, let it scroll, press Esc, get the picture, with nothing drawn over the page that is being captured.

