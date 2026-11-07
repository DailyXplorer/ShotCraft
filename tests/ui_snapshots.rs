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
#[path = "../src/visual.rs"]
mod visual;

use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_shotcraft");

/// A picture to open in the editor, the same on every machine.
fn sample(dir: &Path) -> PathBuf {
    let img = RgbaImage::from_fn(900, 560, |x, y| {
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
    let path = dir.join("sample.png");
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
    let pic = sample(&work);
    let pic = Some(pic.as_path());

    // (name, hook, with a picture open). Every hook is the app's own `SHOTCRAFT_*` test switch.
    let cases: &[(&str, &[(&str, &str)], bool)] = &[
        ("empty-editor", &[], false),
        ("editor", &[], true),
        ("menu-shotcraft", &[("SHOTCRAFT_MENU", "ShotCraft")], true),
        ("menu-capture", &[("SHOTCRAFT_MENU", "Capture")], true),
        ("menu-file", &[("SHOTCRAFT_MENU", "File")], true),
        ("menu-edit", &[("SHOTCRAFT_MENU", "Edit")], true),
        ("menu-draw", &[("SHOTCRAFT_MENU", "Draw")], true),
        ("menu-zoom", &[("SHOTCRAFT_MENU", "Zoom")], true),
        ("menu-help", &[("SHOTCRAFT_MENU", "Help")], true),
        ("menu-file-no-picture", &[("SHOTCRAFT_MENU", "File")], false),
        ("preferences-general", &[("SHOTCRAFT_PREFS_TAB", "0")], false),
        ("preferences-hotkeys", &[("SHOTCRAFT_PREFS_TAB", "1")], false),
        ("preferences-advanced", &[("SHOTCRAFT_PREFS_TAB", "2")], false),
        ("dialog-shortcuts", &[("SHOTCRAFT_DIALOG", "shortcuts")], false),
        ("dialog-about", &[("SHOTCRAFT_DIALOG", "about")], false),
        ("capture-area", &[("SHOTCRAFT_PICKER", "area")], true),
        ("capture-window", &[("SHOTCRAFT_PICKER", "window")], true),
        ("tool-arrow-options", &[("SHOTCRAFT_TOOL", "a")], true),
    ];
    let mut failures = vec![];
    for (name, env, with_picture) in cases {
        match shoot(&work, name, env, with_picture.then_some(pic).flatten()) {
            // A few pixels of leeway for text smoothing; a changed letter, a moved or missing control is more.
            Ok(img) => {
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
