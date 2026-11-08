//! Linux: ShotCraft's launcher entry and icon. A Linux executable has no icon of its own, and GNOME
//! (Wayland) ignores the icon a program sets on its window: it matches the window's app id to a
//! desktop entry and shows that entry's icon. Without an entry the dock, Alt+Tab and the app grid
//! showed a generic icon, and ShotCraft wasn't in the app grid at all.
//!
//! So ShotCraft installs, for this user, `shotcraft.desktop` (named like the window's app id,
//! `APP_ID`) and the icon it points at, the same way it installs its `shotcraft://` handler
//! (`links`). A copy run from cargo's target folder leaves them alone, unless asked
//! (`SHOTCRAFT_INSTALL_LAUNCHER=1`).

use std::path::{Path, PathBuf};

/// The window's app id on Linux: the desktop entry's name without `.desktop`.
pub const APP_ID: &str = "shotcraft";

const ICON: &[u8] = include_bytes!("../assets/window-icon.png");

/// Where the entry and the icon go, under the user's data folder.
fn data_dir() -> PathBuf {
    dirs::data_dir().or_else(|| dirs::home_dir().map(|h| h.join(".local/share"))).unwrap_or_default()
}

/// The visible launcher entry: it starts the app and carries the icon. `StartupWMClass` ties X11
/// windows to it as the app id does on Wayland.
pub fn desktop_entry(exe: &Path) -> String {
    let quoted = crate::links::exec_quoted(exe);
    format!(
        "[Desktop Entry]\nType=Application\nName=ShotCraft\nComment=Screenshots: capture, annotate, blur, measure, read text and pin\nExec=\"{quoted}\"\nIcon={APP_ID}\nCategories=Graphics;\nStartupWMClass={APP_ID}\nTerminal=false\n"
    )
}

/// Whether this copy should install its launcher: not a development build, unless asked.
pub fn wanted(exe: &Path, asked: bool) -> bool {
    asked || !exe.components().any(|c| c.as_os_str() == "target")
}

/// Writes the entry and the icon when they are missing or differ, and tells the desktop about them.
pub fn install() {
    let Ok(exe) = std::env::current_exe() else { return };
    if !wanted(&exe, std::env::var_os("SHOTCRAFT_INSTALL_LAUNCHER").is_some_and(|v| !v.is_empty())) {
        return;
    }
    let base = data_dir();
    let apps = base.join("applications");
    let icons = base.join("icons/hicolor/256x256/apps");
    let (entry_path, icon_path) = (apps.join(format!("{APP_ID}.desktop")), icons.join(format!("{APP_ID}.png")));
    let entry = desktop_entry(&exe);
    let mut changed = false;
    for (path, bytes) in [(&entry_path, entry.as_bytes()), (&icon_path, ICON)] {
        if std::fs::read(path).is_ok_and(|have| have == bytes) {
            continue;
        }
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(path, bytes));
        match written {
            Ok(()) => changed = true,
            Err(e) => eprintln!("launcher: could not write {}: {e}", path.display()),
        }
    }
    if changed {
        // A missing tool only costs the desktops that need it.
        let quiet = |program: &str, args: Vec<std::ffi::OsString>| {
            let _ = std::process::Command::new(program).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        };
        quiet("update-desktop-database", vec![apps.into_os_string()]);
        quiet("gtk-update-icon-cache", vec!["-f".into(), "-t".into(), base.join("icons/hicolor").into_os_string()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_visible_names_the_icon_and_ties_windows_to_it() {
        let e = desktop_entry(Path::new("/opt/Shot \"Craft\"/$bin"));
        assert!(e.contains("Exec=\"/opt/Shot \\\"Craft\\\"/\\$bin\"\n"), "{e}");
        assert!(e.contains("Icon=shotcraft\n") && e.contains("StartupWMClass=shotcraft\n"), "{e}");
        assert!(!e.contains("NoDisplay") && e.contains("Categories=Graphics;\n"), "it shows in the app grid: {e}");
    }

    #[test]
    fn the_bundled_icon_is_a_256_pixel_png() {
        assert_eq!(&ICON[..8], b"\x89PNG\r\n\x1a\n");
        let (w, h) = (u32::from_be_bytes(ICON[16..20].try_into().unwrap()), u32::from_be_bytes(ICON[20..24].try_into().unwrap()));
        assert_eq!((w, h), (256, 256), "the size of the folder it is installed in");
    }

    #[test]
    fn a_development_build_installs_nothing_unless_asked() {
        assert!(!wanted(Path::new("/home/me/ShotCraft/target/release/shotcraft"), false));
        assert!(wanted(Path::new("/home/me/ShotCraft/target/release/shotcraft"), true));
        assert!(wanted(Path::new("/usr/bin/shotcraft"), false));
    }
}
