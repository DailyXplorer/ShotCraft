//! Window-level visual regression tests: the app's own `--screenshot` hooks draw menus, dialogs, the
//! empty editor and the capture crosshair, and each picture is compared with a saved reference.
//!
//! They open real windows (a few seconds each) and the references are made at 100% display scaling
//! with the system's font for symbols, so they are ignored unless asked for:
//!
//!   cargo test --test ui_snapshots -- --ignored
//!
//! References live in `tests/reference/ui-<system>/`; a system without any is skipped. After a change
//! on purpose, rewrite them with `SHOTCRAFT_UPDATE_REFERENCES=1` and look at them before committing.
//! A failure leaves the new picture and a red-marked difference in `target/tmp/visual-diffs/`.
//! `SHOTCRAFT_UI_PICTURES=folder` keeps every picture drawn, passing or not: a build machine has
//! no screen to watch.
#[path = "../src/visual.rs"]
mod visual;

use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_shotcraft");

/// What the editor has open in a picture of the app's windows.
#[derive(Clone, Copy)]
enum Open {
    Nothing,
    /// 900 x 560: the window takes its size.
    Picture,
    /// 300 x 200: the window is the smallest one a picture opens in, 836 x 470 in the unified arrangement.
    SmallPicture,
}

/// A picture to open in the editor, the same on every machine.
fn sample(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let img = RgbaImage::from_fn(width, height, |x, y| {
        let band = (y / 70) % 2 == 0;
        let c = if x > 520 && y > 60 && y < 260 {
            [40, 44, 52]
        } else if band {
            [245, 246, 248]
        } else {
            [228, 233, 240]
        };
        Rgba([c[0], c[1], c[2], 255])
    });
    let path = dir.join(name);
    img.save(&path).expect("write the sample picture");
    path
}

/// Runs the app with `env`, and returns the picture it drew.
fn shoot(dir: &Path, name: &str, env: &[(&str, &str)], picture: Option<&Path>) -> Result<RgbaImage, String> {
    let out = dir.join(format!("{name}.png"));
    let _ = std::fs::remove_file(&out);
    let mut cmd = Command::new(EXE);
    cmd.arg("--screenshot").arg(&out);
    if let Some(p) = picture {
        cmd.arg(p);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{name}: can't start the app: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(60) => {
                let _ = child.kill();
                return Err(format!("{name}: the app didn't finish in 60 s"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
    image::open(&out).map(|i| i.to_rgba8()).map_err(|e| format!("{name}: no picture was written ({e})"))
}

#[test]
#[ignore = "opens windows; references are for 100% display scaling (see the file's comment)"]
fn windows_match_the_reference_pictures() {
    let refs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("reference").join(format!("ui-{}", std::env::consts::OS));
    let updating = std::env::var_os("SHOTCRAFT_UPDATE_REFERENCES").is_some_and(|v| !v.is_empty());
    if !refs.is_dir() && !updating {
        eprintln!("no references for {} ({}): skipped", std::env::consts::OS, refs.display());
        return;
    }
    let work = std::env::temp_dir().join(format!("shotcraft-ui-snapshots-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let (picture, small) = (sample(&work, "sample.png", 900, 560), sample(&work, "small.png", 300, 200));
    let keep = std::env::var_os("SHOTCRAFT_UI_PICTURES").map(PathBuf::from);

    // (name, hook, what is open). Every hook is the app's own `SHOTCRAFT_*` test switch.
    let cases: &[(&str, &[(&str, &str)], Open)] = &[
        ("empty-editor", &[], Open::Nothing),
        ("editor", &[], Open::Picture),
        ("menu-shotcraft", &[("SHOTCRAFT_MENU", "ShotCraft")], Open::Picture),
        ("menu-capture", &[("SHOTCRAFT_MENU", "Capture")], Open::Picture),
        ("menu-file", &[("SHOTCRAFT_MENU", "File")], Open::Picture),
        ("menu-edit", &[("SHOTCRAFT_MENU", "Edit")], Open::Picture),
        ("menu-draw", &[("SHOTCRAFT_MENU", "Draw")], Open::Picture),
        ("menu-zoom", &[("SHOTCRAFT_MENU", "Zoom")], Open::Picture),
        ("menu-help", &[("SHOTCRAFT_MENU", "Help")], Open::Picture),
        ("menu-file-no-picture", &[("SHOTCRAFT_MENU", "File")], Open::Nothing),
        // The longest menu in the smallest window a picture opens in.
        ("menu-draw-small-window", &[("SHOTCRAFT_MENU", "Draw")], Open::SmallPicture),
        ("preferences-general", &[("SHOTCRAFT_PREFS_TAB", "0")], Open::Nothing),
        ("preferences-hotkeys", &[("SHOTCRAFT_PREFS_TAB", "1")], Open::Nothing),
        ("preferences-advanced", &[("SHOTCRAFT_PREFS_TAB", "2")], Open::Nothing),
        ("dialog-shortcuts", &[("SHOTCRAFT_DIALOG", "shortcuts")], Open::Nothing),
        ("dialog-about", &[("SHOTCRAFT_DIALOG", "about")], Open::Nothing),
        ("capture-area", &[("SHOTCRAFT_PICKER", "area")], Open::Picture),
        ("capture-window", &[("SHOTCRAFT_PICKER", "window")], Open::Picture),
        ("tool-arrow-options", &[("SHOTCRAFT_TOOL", "a")], Open::Picture),
    ];
    let mut failures = vec![];
    for (name, env, open) in cases {
        let file = match open {
            Open::Nothing => None,
            Open::Picture => Some(picture.as_path()),
            Open::SmallPicture => Some(small.as_path()),
        };
        match shoot(&work, name, env, file) {
            // A few pixels of leeway for text smoothing; a changed letter, a moved or missing control is more.
            Ok(img) => {
                if let Some(dir) = &keep {
                    std::fs::create_dir_all(dir).expect("the folder for the pictures");
                    img.save(dir.join(format!("{name}.png"))).expect("keep the picture");
                }
                if let Err(e) = visual::check(name, &img, &refs, 8, 0.00001) {
                    failures.push(e);
                }
            }
            Err(e) => failures.push(e),
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
