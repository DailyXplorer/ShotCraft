//! Windows and Linux: how `shotcraft://` links reach the app (macOS: Apple events, `url_scheme`).
//!
//! The system hands a link to a program by starting it with the link as its argument, so the link
//! type has to be registered for this user: under `HKCU\Software\Classes\shotcraft` on Windows, as
//! a desktop entry for `x-scheme-handler/shotcraft` on Linux. macOS registers it in the app bundle;
//! a portable exe has no installer to do it, so ShotCraft registers it itself, and only while
//! Preferences ▸ Advanced ▸ Automation allows links. Switching that off removes the registration.
//!
//! What a link may do is `url_scheme`'s to decide, the same on every system. A copy of ShotCraft
//! that is already running gets the link from the new launch (`single`).

/// A link starts with this, in any case (`ShotCraft://` works too, as on macOS).
const PREFIX: &str = "shotcraft:";

/// The launch's arguments without the link, and the link.
///
/// A launch with a link is only that: every other argument is dropped. Windows builds the command
/// line from `"%1"`, so a link holding a quote would otherwise add options of its own
/// (`--screenshot`, a file to open).
pub fn split_args(args: Vec<String>) -> (Vec<String>, Option<String>) {
    let is_link = |a: &String| a.get(..PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(PREFIX));
    match args.iter().skip(1).find(|a| is_link(a)).cloned() {
        Some(link) => (args.into_iter().take(1).collect(), Some(link)),
        None => (args, None),
    }
}

/// Keeps the registration on this copy of ShotCraft while links are allowed, wherever it was
/// switched on from. A copy run from cargo's target folder (a development build) leaves it alone.
pub fn refresh(allowed: bool) {
    let dev_build = std::env::current_exe().is_ok_and(|e| e.components().any(|c| c.as_os_str() == "target"));
    if allowed && !dev_build {
        set_registered(true);
    }
}

#[cfg(target_os = "windows")]
const KEY: &str = r"Software\Classes\shotcraft";

/// Registers the link type for this user (`on`), or removes the registration.
#[cfg(target_os = "windows")]
pub fn set_registered(on: bool) {
    let result = match (on, std::env::current_exe()) {
        (false, _) => unregister_at(KEY),
        (true, Ok(exe)) => register_at(KEY, &exe),
        (true, Err(e)) => Err(e),
    };
    if let Err(e) = result {
        eprintln!("shotcraft:// links: the registration could not be changed: {e}");
    }
}

/// Writes the registration under `key` (in HKCU): the scheme, its icon and the command.
#[cfg(target_os = "windows")]
fn register_at(key: &str, exe: &std::path::Path) -> std::io::Result<()> {
    let (k, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).create_subkey(key)?;
    k.set_value("", &"URL:ShotCraft link")?;
    k.set_value("URL Protocol", &"")?;
    k.create_subkey("DefaultIcon")?.0.set_value("", &format!("\"{}\",0", exe.display()))?;
    k.create_subkey(r"shell\open\command")?.0.set_value("", &command(exe))
}

/// Removes the registration under `key`, only when it starts ShotCraft: one another program made stays.
#[cfg(target_os = "windows")]
fn unregister_at(key: &str) -> std::io::Result<()> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let command: Option<String> = hkcu.open_subkey(format!(r"{key}\shell\open\command")).and_then(|k| k.get_value("")).ok();
    if command.is_some_and(|c| is_ours(&c)) {
        hkcu.delete_subkey_all(key)?;
    }
    Ok(())
}

/// What Windows runs for a link: ShotCraft with the link as its one argument.
#[cfg(target_os = "windows")]
fn command(exe: &std::path::Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

#[cfg(target_os = "windows")]
fn is_ours(command: &str) -> bool {
    command.to_ascii_lowercase().contains("shotcraft")
}

/// The desktop entry's file name, which `xdg-mime` takes to make it the handler.
#[cfg(target_os = "linux")]
const DESKTOP_FILE: &str = "shotcraft-links.desktop";

/// Registers the link type for this user (`on`), or removes the registration.
#[cfg(target_os = "linux")]
pub fn set_registered(on: bool) {
    let dir = desktop_dir();
    let path = dir.join(DESKTOP_FILE);
    if on {
        let Ok(exe) = std::env::current_exe() else { return };
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, desktop_entry(&exe)));
        if let Err(e) = written {
            eprintln!("shotcraft:// links: could not register them: {e}");
            return;
        }
    } else {
        // The default left in mimeapps.list names a file that is gone: desktops skip it.
        let _ = std::fs::remove_file(path);
    }
    // A missing tool only costs the desktops that need it.
    for (program, args) in registration_commands(&dir, on) {
        let _ = std::process::Command::new(program).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// What tells the desktop about the entry after it was written (`on`) or removed. `xdg-mime` makes it
/// the handler (xdg-utils is on every freedesktop system). `update-desktop-database` rebuilds the
/// folder's mimeinfo.cache: without it the entry is the default but not a registered handler, and the
/// "Open With" dialog of the desktop portal, which sandboxed browsers (Firefox as a snap) go through,
/// offered no app for a shotcraft:// link.
#[cfg(target_os = "linux")]
fn registration_commands(dir: &std::path::Path, on: bool) -> Vec<(&'static str, Vec<std::ffi::OsString>)> {
    let mut out = vec![];
    if on {
        out.push(("xdg-mime", ["default", DESKTOP_FILE, "x-scheme-handler/shotcraft"].map(Into::into).to_vec()));
    }
    out.push(("update-desktop-database", vec![dir.as_os_str().to_owned()]));
    out
}

#[cfg(target_os = "linux")]
fn desktop_dir() -> std::path::PathBuf {
    dirs::data_dir().or_else(|| dirs::home_dir().map(|h| h.join(".local/share"))).unwrap_or_default().join("applications")
}

/// A desktop entry that handles `shotcraft://` and shows in no menu (`%u`: the link).
#[cfg(target_os = "linux")]
fn desktop_entry(exe: &std::path::Path) -> String {
    let program = exec_program(exe);
    format!("[Desktop Entry]\nType=Application\nName=ShotCraft\nComment=Opens shotcraft:// links\nExec={program} %u\nMimeType=x-scheme-handler/shotcraft;\nNoDisplay=true\nTerminal=false\n")
}

/// A path as a desktop entry's Exec line wants it (freedesktop Desktop Entry spec): backslash,
/// quote, backtick and $ are escaped, and a literal % is written %% (a lone % starts a field code
/// such as %u, which would be replaced by the link).
#[cfg(target_os = "linux")]
pub fn exec_quoted(exe: &std::path::Path) -> String {
    exe.display()
        .to_string()
        .chars()
        .flat_map(|c| match c {
            '\\' | '"' | '`' | '$' => vec!['\\', c],
            '%' => vec!['%', '%'],
            _ => vec![c],
        })
        .collect()
}

/// The program part of an Exec line: the quoted path, or, when the path has a % in it, `env` and the
/// quoted path. GLib (GNOME's launcher library) checks that the Exec's program exists using the
/// text as written, %% and all, and a path like `/home/u/Test 100%%/shotcraft` doesn't exist, so it
/// refuses to load the whole entry (the link handler, the launcher, the login item). `env` does
/// exist, and the %% becomes % when the entry is launched, as the spec says.
#[cfg(target_os = "linux")]
pub fn exec_program(exe: &std::path::Path) -> String {
    let quoted = exec_quoted(exe);
    if exe.to_string_lossy().contains('%') {
        format!("env \"{quoted}\"")
    } else {
        format!("\"{quoted}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_launch_without_a_link_keeps_its_arguments() {
        let a = args(&["shotcraft", "--background"]);
        assert_eq!(split_args(a.clone()), (a, None));
        let a = args(&["shotcraft", "C:\\shots\\a.png"]);
        assert_eq!(split_args(a.clone()), (a, None));
        assert_eq!(split_args(args(&["shotcraft"])), (args(&["shotcraft"]), None));
    }

    #[test]
    fn a_link_launch_is_only_the_link() {
        let (rest, link) = split_args(args(&["shotcraft", "shotcraft://grab/area"]));
        assert_eq!((rest, link.as_deref()), (args(&["shotcraft"]), Some("shotcraft://grab/area")));
        // Any case, as the system passes it on.
        assert_eq!(split_args(args(&["shotcraft", "ShotCraft://show"])).1.as_deref(), Some("ShotCraft://show"));
        // What a link with a quote turns into on Windows: the extra arguments it smuggled in are dropped.
        let (rest, link) = split_args(args(&["shotcraft", "shotcraft://show", "--screenshot", "C:\\x.png", "C:\\y.png"]));
        assert_eq!((rest, link.as_deref()), (args(&["shotcraft"]), Some("shotcraft://show")));
        // The program's own name is never taken for a link.
        assert_eq!(split_args(args(&["shotcraft:"])).1, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_runs_shotcraft_with_the_link_as_its_one_argument() {
        let exe = std::path::Path::new(r"C:\Program Files\ShotCraft\shotcraft.exe");
        assert_eq!(command(exe), r#""C:\Program Files\ShotCraft\shotcraft.exe" "%1""#);
        assert!(is_ours(&command(exe)));
        assert!(!is_ours(r#""C:\Tools\other.exe" "%1""#), "another program's registration stays");
    }

    /// Writes and removes a registration under a key of its own (not under Classes, so it
    /// registers nothing), and leaves another program's registration alone.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_registration_is_written_read_back_and_removed() {
        use winreg::enums::HKEY_CURRENT_USER;
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let key = format!(r"Software\ShotCraft-tests\links-{}", std::process::id());
        let exe = std::path::Path::new(r"C:\Tools\ShotCraft\shotcraft.exe");
        register_at(&key, exe).expect("written");
        let k = hkcu.open_subkey(&key).expect("key");
        assert_eq!(k.get_value::<String, _>("URL Protocol").expect("marker"), "");
        let cmd: String = k.open_subkey(r"shell\open\command").and_then(|c| c.get_value("")).expect("command");
        assert_eq!(cmd, command(exe));
        drop(k);
        unregister_at(&key).expect("removed");
        assert!(hkcu.open_subkey(&key).is_err(), "the registration is gone");
        // Someone else's: kept.
        let (other, _) = hkcu.create_subkey(format!(r"{key}\shell\open\command")).expect("other");
        other.set_value("", &r#""C:\Tools\other.exe" "%1""#).expect("other command");
        drop(other);
        unregister_at(&key).expect("nothing to do");
        assert!(hkcu.open_subkey(&key).is_ok(), "another program's registration stays");
        let _ = hkcu.delete_subkey_all(&key);
        let _ = hkcu.delete_subkey(r"Software\ShotCraft-tests");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_desktop_entry_handles_the_links_and_quotes_the_path() {
        let entry = desktop_entry(std::path::Path::new("/opt/Shot \"Craft\"/$bin"));
        assert!(entry.contains("Exec=\"/opt/Shot \\\"Craft\\\"/\\$bin\" %u\n"), "{entry}");
        assert!(entry.contains("MimeType=x-scheme-handler/shotcraft;\n"));
        assert!(entry.contains("NoDisplay=true\n"));
    }

    /// A % in the path is a literal % in an Exec line only as %%: a lone one starts a field code.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_percent_in_the_path_is_doubled_in_both_entries() {
        let path = std::path::Path::new("/home/u/100%/Shot%uCraft");
        assert_eq!(exec_quoted(path), "/home/u/100%%/Shot%%uCraft");
        assert!(desktop_entry(path).contains("Exec=env \"/home/u/100%%/Shot%%uCraft\" %u\n"), "{}", desktop_entry(path));
        // Without a % the program is the path itself, as before.
        assert_eq!(exec_program(std::path::Path::new("/opt/shotcraft")), "\"/opt/shotcraft\"");
        assert!(exec_program(path).starts_with("env "), "GLib needs a program that exists to load the entry");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registering_and_removing_both_rebuild_the_desktop_database() {
        let dir = std::path::Path::new("/home/u/.local/share/applications");
        let rebuild = ("update-desktop-database", vec![dir.as_os_str().to_owned()]);
        let on = registration_commands(dir, true);
        assert_eq!(on[0].0, "xdg-mime", "the entry becomes the default first");
        assert!(on.contains(&rebuild), "and is registered, for the portal's Open With: {on:?}");
        assert_eq!(registration_commands(dir, false), vec![rebuild], "a removed entry leaves the cache too");
    }
}
