# Product decisions

Rules that shape the app. Change one only on purpose, and say so in the pull request.

## Menu bar only: nothing opens at launch

Starting ShotCraft (Finder, `open`, a login item) shows the menu bar icon and no window. A window appears only when the user acts: a capture, opening a file, Preferences, a pin, an update dialog. Starting with an image file as argument opens it in the editor. Opening the app again while it runs does not open the editor.

Why: a capture tool runs all day and must not take over the screen at login.

## No Dock icon

`LSUIElement` is true in `packaging/Info.plist`, and the app sets the accessory activation policy before it creates a window, so a build run outside a bundle behaves the same. The consequences are part of the rule:

* There is no main menu bar. Every command that used to live only there is in the menu bar icon menu.
* Every window that opens activates the app and comes to the front, since the system does not do it for an accessory app. This includes Sparkle's dialogs.
* Editor shortcuts must keep working without a main menu. Anything bound to a menu key equivalent has to be handled in the editor as well.
* The app has no Cmd+Tab entry. This is accepted.
* The menu bar icon can be hidden only while the Show ShotCraft hotkey is set, so the app always stays reachable.
