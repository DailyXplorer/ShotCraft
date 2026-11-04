//! Pins and the thumbnail drawn headless: the wheel over a pin, the hover buttons of the thumbnail.
//! With `SHOTCRAFT_TEST_OUT` set they also write PNGs for a visual check.
use crate::headless::{save_png, window_capture, with_rig};
use crate::{model::Image, sizing, thumb_window, pin_window, Pin, Thumb, Queue};
use eframe::egui::{self, pos2, Event, Modifiers, MouseWheelUnit, TouchPhase};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 900.0);

fn picture() -> Image {
    // A window capture at 2x: 500 x 340 points of it, shadow margin included.
    window_capture(440, 250, 2)
}

fn new_pin(img: Image) -> Pin {
    Pin {
        id: 1,
        img,
        scale: 2.0,
        zoom: 1.0,
        semi: false,
        scroll: 0.0,
        screen: SCREEN,
        wheel: sizing::WheelZoom::default(),
        reverse_wheel: false,
        tex: None,
        open: true,
    }
}

fn queue() -> Queue {
    Arc::new(Mutex::new(Vec::new()))
}

fn notch(dy: f32, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos2(60.0, 60.0)),
        Event::MouseWheel { unit: MouseWheelUnit::Line, delta: egui::vec2(0.0, dy), phase: TouchPhase::Move, modifiers },
    ]
}

fn out_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("SHOTCRAFT_TEST_OUT").map(|d| std::path::PathBuf::from(d))
}

/// Draws the pin in a window of its own size, with `events` delivered.
fn draw_pin(pin: &mut Pin, events: Vec<Event>) -> Image {
    let size = pin.layout().window;
    let q = queue();
    with_rig(|rig| rig.render_window(size, 2.0, events, |ui| pin_window(ui, pin, &q)))
}

#[test]
fn the_wheel_over_a_pin_resizes_it_in_steps_and_keeps_the_aspect() {
    let mut pin = new_pin(picture());
    let aspect = pin.layout().window.0 / pin.layout().window.1;
    let before = pin.layout().window;
    let shot0 = draw_pin(&mut pin, vec![]);
    let mut widths = vec![before.0];
    for _ in 0..3 {
        draw_pin(&mut pin, notch(1.0, Modifiers::NONE));
        let w = pin.layout().window;
        widths.push(w.0);
        assert!((w.0 / w.1 - aspect).abs() < 1e-3, "aspect kept");
    }
    assert!(widths.windows(2).all(|p| p[1] > p[0]), "zoomed in each notch: {widths:?}");
    assert_eq!(pin.zoom, 1.75);
    let shot3 = draw_pin(&mut pin, vec![]);
    for _ in 0..20 {
        draw_pin(&mut pin, notch(-1.0, Modifiers::NONE));
    }
    assert_eq!(pin.zoom, 0.2, "zoom out stops at the minimum width");
    assert!(pin.layout().window.0 <= sizing::PIN_MIN_WIDTH + 0.01);
    let shot_small = draw_pin(&mut pin, vec![]);
    if let Some(dir) = out_dir() {
        save_png(&shot0, &dir.join("pin-100.png"));
        save_png(&shot3, &dir.join("pin-wheel-3-notches-up.png"));
        save_png(&shot_small, &dir.join("pin-wheel-out.png"));
    }
}

#[test]
fn reversed_wheel_preference_flips_the_direction() {
    let mut pin = new_pin(picture());
    pin.reverse_wheel = true;
    draw_pin(&mut pin, notch(1.0, Modifiers::NONE));
    assert!(pin.zoom < 1.0);
}

#[test]
fn a_tall_pin_scrolls_with_the_wheel_and_zooms_with_option() {
    let tall = Image { w: 600, h: 6000, px: Arc::new(vec![180; 600 * 6000 * 4]) };
    let mut pin = new_pin(tall);
    assert!(pin.layout().max_scroll() > 0.0);
    draw_pin(&mut pin, notch(-1.0, Modifiers::NONE));
    assert_eq!(pin.zoom, 1.0, "plain wheel does not resize a pin that has to scroll");
    assert!(pin.scroll > 0.0, "it scrolled");
    let scrolled = pin.scroll;
    draw_pin(&mut pin, notch(1.0, Modifiers { alt: true, ..Modifiers::NONE }));
    assert!(pin.zoom > 1.0, "option + wheel resizes it");
    assert!(pin.scroll > scrolled, "keeping the same part of the picture on top");
}

#[test]
fn a_normal_pin_zoomed_past_the_screen_height_still_zooms_and_scrolls_with_option() {
    let mut pin = new_pin(picture());
    for _ in 0..60 {
        draw_pin(&mut pin, notch(1.0, Modifiers::NONE));
    }
    assert!(pin.zoom >= 2.0);
    let before = pin.zoom;
    draw_pin(&mut pin, notch(-1.0, Modifiers::NONE));
    assert!(pin.zoom < before, "the wheel keeps zooming");
    // zoom in far enough that the window is capped in height and the picture has to scroll
    let mut tall_zoom = new_pin(window_capture(100, 300, 2));
    tall_zoom.zoom = 3.0;
    assert!(tall_zoom.layout().max_scroll() > 0.0);
    draw_pin(&mut tall_zoom, notch(-1.0, Modifiers { alt: true, ..Modifiers::NONE }));
    assert!(tall_zoom.scroll > 0.0, "option + wheel scrolls it");
    assert_eq!(tall_zoom.zoom, 3.0);
}

#[test]
fn a_pin_never_outgrows_the_screen() {
    let mut pin = new_pin(picture());
    for _ in 0..60 {
        draw_pin(&mut pin, notch(1.0, Modifiers::NONE));
    }
    let w = pin.layout().window;
    assert!(w.0 <= SCREEN.x * sizing::PIN_SCREEN_FRACTION + 0.01 && w.1 <= SCREEN.y * sizing::PIN_SCREEN_FRACTION + 0.01);
    // and the next notch down shrinks what is on screen, not a hidden zoom
    let shown = w.0;
    draw_pin(&mut pin, notch(-1.0, Modifiers::NONE));
    assert!(pin.layout().window.0 < shown);
}

#[test]
fn thumbnail_hover_buttons() {
    let img = picture();
    let layout = sizing::thumb_layout((img.w, img.h), crate::model::THUMB_WIDTH, crate::model::THUMB_MAX_HEIGHT);
    let mut th = Thumb { img, scale: 2.0, tex: None, born: Instant::now(), hovered: false, focused: false, blurred: false };
    let q = queue();
    let still = with_rig(|rig| rig.render_window(layout.window, 2.0, vec![Event::PointerGone], |ui| thumb_window(ui, &mut th, &q)));
    assert!(!th.hovered);
    let c = layout.window;
    let over = with_rig(|rig| rig.render_window(layout.window, 2.0, vec![Event::PointerMoved(pos2(c.0 / 2.0, c.1 - 10.0))], |ui| thumb_window(ui, &mut th, &q)));
    assert!(th.hovered);
    assert_eq!((still.w, still.h), (204 * 2, (layout.window.1 * 2.0) as u32));
    if let Some(dir) = out_dir() {
        save_png(&still, &dir.join("thumb.png"));
        save_png(&over, &dir.join("thumb-hover.png"));
    }
}
