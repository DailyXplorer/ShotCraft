//! Renders of the Add Capture / pasted picture UI for the eyes (`SHOTCRAFT_TEST_OUT`), plus the
//! checks that need the whole editor.

use crate::headless::{save_png, with_rig};
use crate::model::*;
use std::sync::Arc;

fn page(w: u32, h: u32, tint: [u8; 3]) -> Image {
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let c = if y < 28 { [60, 63, 70] } else if (x / 40 + y / 40) % 2 == 0 { [248, 248, 250] } else { tint };
            px.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    Image { w, h, px: Arc::new(px) }
}

fn out_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("SHOTCRAFT_TEST_OUT").map(|d| std::path::PathBuf::from(d).join("addcap"))
}

#[test]
fn pasted_picture_and_added_capture_for_the_eyes() {
    let size = (1100.0, 760.0);
    let ppp = 2.0;
    let shots = with_rig(|rig| {
        let mut app = rig.app();
        let mut doc = Doc::new(page(420, 260, [226, 232, 242]), ppp);
        let i = doc.paste_image(page(160, 120, [255, 200, 120]));
        if let Kind::Image { opacity, .. } = &mut doc.objs[i].kind {
            *opacity = 0.7;
        }
        doc.objs[i].translate(40.0, 10.0);
        app.doc = Some(doc);
        app.select_object(i);
        let selected = rig.render_editor(&mut app, size, ppp, false);
        // Add Capture: a second, shorter picture beside the first.
        let d = app.doc.as_mut().unwrap();
        d.append_capture(&page(300, 180, [200, 235, 210]));
        app.canvas_changed();
        let added = rig.render_editor(&mut app, size, ppp, false);
        (selected, added)
    });
    // The options bar appears for a selected picture: the first render is not just the plain editor.
    assert!(shots.0.px != shots.1.px);
    if let Some(dir) = out_dir() {
        save_png(&shots.0, &dir.join("overlay-selected.png"));
        save_png(&shots.1, &dir.join("add-capture.png"));
    }
}
