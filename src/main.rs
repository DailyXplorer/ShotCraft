mod app_mode;
mod color;
mod editor;
mod frozen;
mod frozen_ui;
#[cfg(test)]
mod headless;
#[cfg(test)]
mod addcap_shots;
mod icons;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod menu_model;
mod model;
mod prefs;
mod render;
mod autoscroll;
mod scroll;
mod selection;
mod snap;
mod scroll_control;
mod settings;
mod sizing;
mod tray_flash;
#[cfg(test)]
mod pin_shots;
mod stitch;
mod tiled;
#[cfg(target_os = "macos")]
mod shell;
#[cfg(target_os = "macos")]
mod updater;
mod system;
mod targets;
mod windowbg;

use eframe::egui::{self, Color32, ViewportBuilder, ViewportCommand, ViewportId};
use model::{Doc, Image, Tool};
use render::Fonts;
use settings::{AfterShow, Confirm, Settings};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use system::{CaptureMode, Captured};

pub enum Action {
    Capture(CaptureMode),
    InstantOcr,
    OpenClipboard,
    OpenFile,
    Show,
    Preferences,
    ToggleAutostart,
    CheckForUpdates,
    ToggleAutoUpdates,
    /// Draw menu: Snap to Objects (false) or Snap to Similar Objects (true).
    ToggleSnap(bool),
    Quit,
    Copy,
    Save,
    SaveAs,
    Pin,
    Print,
    Close,
    CloseAll,
    Undo,
    Redo,
    ResetCrop,
    Rasterize,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    Zoom100,
    ZoomSelection,
    SelectionTopLeft,
    SelectionBottomRight,
    About,
    Shortcuts,
    Tool(Tool),
    /// Edit ▸ Paste: the clipboard picture goes over the screenshot as a movable object.
    Paste,
    /// Draw ▸ Add Capture: grab another area and put it beside the current picture.
    AddCapture,
    /// Saves the before/after GIF of the pasted picture to the screenshots folder.
    SaveGif,
    PickFolder,
    PinEdit(Image, f32),
    PinCopy(Image),
    ThumbDo(ThumbAction),
    Captured(Result<Option<Captured>, String>),
    ScrollingCapture,
    /// Scrolling capture that starts at the bottom of the page and goes up.
    ScrollingCaptureUp,
    ScrollingCancel,
    /// Return during a scrolling capture: ends it. Unlike `ScrollingCapture` it never starts one.
    ScrollingFinish,
    ScrollingArea(Result<Option<model::R>, String>),
    ScrollingFinished(Result<Option<scroll::Outcome>, String>),
    OcrCaptured(Result<Option<Captured>, String>),
    OcrDone(Result<system::OcrResult, String>),
    /// The screens are photographed: the frozen area grabber can open its windows.
    FrozenShots(Box<FrozenStart>),
}

/// What the frozen area grabber starts from.
pub struct FrozenStart {
    shots: Vec<frozen::Shot>,
    /// Kept for the fall back to the system selector.
    opt: system::CaptureOptions,
    for_ocr: bool,
}

/// A frozen area grab under way.
struct Frozen {
    shared: frozen_ui::Shared,
    opt: system::CaptureOptions,
    /// The display under the mouse when the grab began: its window takes the keyboard.
    key: usize,
    /// The windows are above the menu bar and in front.
    raised: bool,
}

pub type Queue = Arc<Mutex<Vec<Action>>>;

#[derive(Clone, Copy)]
pub enum ThumbAction {
    Edit,
    Copy,
    Save,
    Ocr,
    Pin,
    Close,
}

/// Shows a floating window as a deferred viewport. Unlike immediate viewports, eframe can create
/// these while the editor is hidden; the extra repaint lets it build the window on the next pass.
pub fn show_window(ctx: &egui::Context, id: ViewportId, builder: ViewportBuilder, body: impl Fn(&mut egui::Ui) + Send + Sync + 'static) {
    if !ctx.input(|i| i.raw.viewports.contains_key(&id)) {
        ctx.request_repaint();
    }
    ctx.show_viewport_deferred(id, builder, move |ui, _| body(ui));
}

fn send(ctx: &egui::Context, queue: &Queue, action: Action) {
    queue.lock().unwrap().push(action);
    ctx.request_repaint_of(ViewportId::ROOT);
}

/// Title of the editor window; the other windows have their own.
pub const EDITOR_TITLE: &str = "ShotCraft";

/// How long the editor stays on screen, transparent, before it is hidden again.
const HIDE_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

const WINDOW_MARGIN: f32 = 48.0;
const WINDOW_MIN_HEIGHT_RATIO: f32 = 0.9;

pub struct Pin {
    id: u64,
    img: Image,
    scale: f32,
    zoom: f32,
    semi: bool,
    /// How far the picture is scrolled up, in points, when it is taller than the window.
    scroll: f32,
    /// Screen the window is sized for. Set by `pins_ui` only, so the window, the picture and the
    /// scroll range always agree, wherever the pin is dragged to.
    screen: egui::Vec2,
    /// Turns wheel events over the pin into zoom steps.
    wheel: sizing::WheelZoom,
    /// The "Zoom with mouse wheel" preference, copied here by `pins_ui` like `screen`.
    reverse_wheel: bool,
    tex: Option<tiled::TiledTexture>,
    open: bool,
}

pub struct Thumb {
    img: Image,
    scale: f32,
    tex: Option<tiled::TiledTexture>,
    born: Instant,
    hovered: bool,
    focused: bool,
    blurred: bool,
}

pub struct App {
    pub queue: Queue,
    pub settings: Settings,
    pub fonts: Arc<Fonts>,
    #[cfg(target_os = "macos")]
    shell: Option<shell::Shell>,
    pub doc: Option<Doc>,
    pub ed: editor::Editor,
    pins: Vec<Arc<Mutex<Pin>>>,
    next_pin: u64,
    thumb: Option<Arc<Mutex<Thumb>>>,
    pub prefs: Arc<Mutex<prefs::PrefsState>>,
    pub toast: Option<(String, f64)>,
    screenshot: Option<(PathBuf, u32)>,
    open_at_start: Option<PathBuf>,
    started: bool,
    quitting: bool,
    /// A hotkey is being recorded in Preferences: menu key equivalents and global hotkeys are off.
    keys_suspended: bool,
    /// The editor window is not on screen. True from launch until the user asks for it.
    pub hidden: bool,
    /// The editor is on screen, transparent, and will be hidden again: eframe's first paint shows it.
    startup_hide: bool,
    /// A brief invisible show of the editor is under way, so that eframe runs the pass that
    /// creates a new window (see `wake_ui`).
    waking: bool,
    /// When the editor was last made invisible-but-on-screen (startup or `wake_ui`).
    woke_at: Instant,
    /// Passes left in which the app and its windows are brought to the front.
    front_until: Option<Instant>,
    front_title: &'static str,
    front_focus: bool,
    scroll: Option<scroll::Session>,
    frozen: Option<Frozen>,
    /// Which way the scrolling capture being set up goes.
    scroll_dir: scroll::Direction,
    ocr_running: bool,
    busy: bool,
    /// The capture under way adds to the current picture (Add Capture) instead of replacing it.
    appending: bool,
    pub about_open: bool,
    pub shortcuts_open: bool,
    ctx: egui::Context,
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("dm-sans".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/DMSans-Regular.ttf"))));
    fonts.font_data.insert("dm-sans-medium".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/DMSans-Medium.ttf"))));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "dm-sans".into());
    // DM Sans has no ⌘ ⇧ ⌥ ⌃ ⎋ ⏎ or arrow glyphs. Three tiny subsets of Noto Symbols, Symbols 2 and Math
    // (SIL OFL, `assets/fonts/OFL-Noto.txt`) cover them on every machine, so they never show as boxes.
    for (at, (name, bytes)) in [
        ("symbols-2", &include_bytes!("../assets/fonts/NotoSymbols2-Subset.ttf")[..]),
        ("symbols-1", &include_bytes!("../assets/fonts/NotoSymbols-Subset.ttf")[..]),
        ("symbols-math", &include_bytes!("../assets/fonts/NotoMath-Subset.ttf")[..]),
    ]
    .into_iter()
    .enumerate()
    {
        fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
        // Right after DM Sans, ahead of egui's built-in fallbacks, so the glyphs look the same everywhere.
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(1 + at, name.into());
    }
    fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "dm-sans".into());
    let mut medium = vec!["dm-sans-medium".to_string()];
    medium.extend(fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(egui::FontFamily::Name("medium".into()), medium);
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|s| {
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Heading, FontId::new(17.0, FontFamily::Name("medium".into())));
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
    });
}

impl App {
    /// An app with no window, for tests that draw the editor.
    #[cfg(test)]
    pub fn headless(ctx: egui::Context) -> App {
        let settings = Settings::default();
        let queue = Queue::default();
        App {
            prefs: prefs::PrefsState::shared(settings.clone(), queue.clone()),
            queue,
            fonts: Arc::new(Fonts::load()),
            #[cfg(target_os = "macos")]
            shell: None,
            doc: None,
            ed: editor::Editor::default(),
            pins: vec![],
            next_pin: 1,
            thumb: None,
            toast: None,
            screenshot: None,
            open_at_start: None,
            started: true,
            quitting: false,
            keys_suspended: false,
            hidden: false,
            startup_hide: false,
            waking: false,
            woke_at: Instant::now(),
            front_until: None,
            front_title: "",
            front_focus: true,
            scroll: None,
            frozen: None,
            scroll_dir: scroll::Direction::Down,
            ocr_running: false,
            busy: false,
            appending: false,
            about_open: false,
            shortcuts_open: false,
            ctx,
            settings,
        }
    }

    fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, show_at_start: bool, open_at_start: Option<PathBuf>) -> App {
        setup_fonts(&cc.egui_ctx);
        let settings = Settings::load();
        system::refresh_autostart(settings.autostart);
        let queue = Queue::default();
        App {
            prefs: prefs::PrefsState::shared(settings.clone(), queue.clone()),
            queue,
            fonts: Arc::new(Fonts::load()),
            #[cfg(target_os = "macos")]
            shell: None,
            doc: None,
            ed: editor::Editor::from_settings(&settings),
            pins: vec![],
            next_pin: 1,
            thumb: None,
            toast: None,
            screenshot: screenshot.map(|p| (p, 0)),
            open_at_start,
            started: false,
            quitting: false,
            keys_suspended: false,
            hidden: !show_at_start,
            startup_hide: false,
            waking: false,
            woke_at: Instant::now(),
            front_until: None,
            front_title: "",
            front_focus: true,
            scroll: None,
            frozen: None,
            scroll_dir: scroll::Direction::Down,
            ocr_running: false,
            busy: false,
            appending: false,
            about_open: false,
            shortcuts_open: false,
            ctx: cc.egui_ctx.clone(),
            settings,
        }
    }

    pub fn push(&self, a: Action) {
        self.queue.lock().unwrap().push(a);
        self.ctx.request_repaint();
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        match self.settings.confirmation {
            Confirm::None => {}
            Confirm::System => system::notify("ShotCraft", &msg),
            Confirm::Custom => {
                if self.hidden {
                    system::notify("ShotCraft", &msg);
                } else {
                    let now = self.ctx.input(|i| i.time);
                    self.toast = Some((msg, now + 2.2));
                }
            }
        }
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.hidden {
            system::notify("ShotCraft", &msg);
        } else {
            let now = self.ctx.input(|i| i.time);
            self.toast = Some((msg, now + 4.0));
        }
    }

    fn show_window(&mut self) {
        self.hidden = false;
        self.waking = false;
        self.startup_hide = false;
        app_mode::set_editor_alpha(1.0);
        self.ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
        self.bring_to_front(EDITOR_TITLE);
    }

    fn hide_window(&mut self) {
        self.hidden = true;
        self.ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(false));
        self.ctx.request_repaint();
        self.sync_menu_state();
    }

    /// Activates the app and puts its windows in front over the next few frames, the window with
    /// this title in focus. An app without a Dock icon is never activated by the system.
    fn bring_to_front(&mut self, title: &'static str) {
        app_mode::activate();
        self.front_until = Some(Instant::now() + std::time::Duration::from_secs(3));
        self.front_title = title;
        self.front_focus = true;
        self.ctx.request_repaint();
    }

    /// Like `bring_to_front`, but the app is not activated and no window takes the keyboard: for a HUD
    /// that floats over another app the user keeps working in.
    fn show_without_focus(&mut self, title: &'static str) {
        self.front_until = Some(Instant::now() + std::time::Duration::from_secs(3));
        self.front_title = title;
        self.front_focus = false;
        self.ctx.request_repaint();
    }

    /// While the editor is hidden eframe runs no UI pass, and a new window (Preferences, a pin,
    /// the thumbnail) can only be created in one. So the editor is shown for a few frames,
    /// fully transparent, and hidden again once the new window exists; the windows that are open
    /// keep the passes running after that.
    fn wake_ui(&mut self) {
        if !self.hidden {
            return;
        }
        app_mode::set_editor_alpha(0.0);
        self.waking = true;
        self.woke_at = Instant::now();
        self.ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(true));
        self.ctx.request_repaint();
    }

    /// End of every UI pass: the delayed steps of the three helpers above.
    fn settle_windows(&mut self) {
        if let Some(until) = self.front_until {
            // The window may not exist yet; keep at it until it is in front, or for a few seconds.
            if app_mode::bring_to_front(self.front_title, self.front_focus) || Instant::now() > until {
                self.front_until = None;
            } else {
                self.ctx.request_repaint_after(std::time::Duration::from_millis(40));
            }
        }
        // The editor stays up, transparent, for a minimum time, so that eframe has shown it (at
        // startup) or created the new window (see `wake_ui`) before it is hidden.
        if self.startup_hide || self.waking {
            if self.woke_at.elapsed() >= HIDE_DELAY {
                self.startup_hide = false;
                self.waking = false;
                if self.hidden {
                    self.hide_window();
                }
            } else {
                self.ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
    }

    fn pixels_per_point(&self) -> f32 {
        self.ctx.pixels_per_point()
    }

    fn start_capture(&mut self, mode: CaptureMode, for_ocr: bool) {
        if self.busy {
            return;
        }
        self.busy = true;
        let snapshot = system::snapshot(mode);
        let was_visible = !self.hidden;
        if was_visible {
            self.hide_window();
        }
        let solid = model::hex(&self.settings.window_solid);
        let opt = system::CaptureOptions {
            last_area: self.settings.last_area.map(|a| model::R { x0: a[0], y0: a[1], x1: a[2], y1: a[3] }),
            window_bg: self.settings.window_bg,
            solid_color: [solid[0], solid[1], solid[2]],
            scale: self.pixels_per_point(),
            cursor: self.settings.capture_cursor,
            delay_secs: self.settings.delay(),
            snapshot,
        };
        let q = self.queue.clone();
        let ctx = self.ctx.clone();
        let frozen = mode == CaptureMode::Area && frozen::wanted(self.settings.frozen_area, &opt.snapshot.displays);
        std::thread::spawn(move || {
            if was_visible {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            if frozen {
                // Photograph every display before any window of ours is up. When that fails the
                // system selector does the job as it always did.
                match system::grab_displays(&opt) {
                    Ok(shots) => {
                        q.lock().unwrap().push(Action::FrozenShots(Box::new(FrozenStart { shots, opt, for_ocr })));
                        ctx.request_repaint();
                        return;
                    }
                    Err(e) => eprintln!("frozen area capture unavailable, using the system selector: {e}"),
                }
            }
            let res = system::capture(mode, &opt);
            q.lock().unwrap().push(if for_ocr { Action::OcrCaptured(res) } else { Action::Captured(res) });
            ctx.request_repaint();
        });
    }

    /// The pictures are taken: open one window per display and let the user draw on them.
    fn frozen_start(&mut self, start: FrozenStart) {
        let snap = &start.opt.snapshot;
        let key = snap.mouse.and_then(|m| targets::display_at(&snap.displays, m)).unwrap_or(0).min(start.shots.len().saturating_sub(1));
        let shared = Arc::new(Mutex::new(frozen_ui::State::new(start.shots, start.for_ocr)));
        self.frozen = Some(Frozen { shared, opt: start.opt, key, raised: false });
        // The windows are new and the app has no Dock icon: wake the UI pass that creates them,
        // and give the keyboard to the one under the mouse.
        self.wake_ui();
        self.bring_to_front(frozen_ui::TITLES[key]);
    }

    /// Every pass while a frozen grab runs: shows its windows, and ends it when it is over.
    fn frozen_ui(&mut self, ctx: &egui::Context) {
        let Some(f) = &mut self.frozen else { return };
        let (outcome, timed_out, frames) = {
            let st = f.shared.lock().unwrap();
            (st.outcome(), st.timed_out(), st.grab.frames().to_vec())
        };
        if let Some(out) = outcome {
            return self.frozen_done(out);
        }
        if timed_out {
            // No window came up: the system selector takes over.
            if let Some(f) = self.frozen.take() {
                let for_ocr = f.shared.lock().unwrap().for_ocr;
                eprintln!("frozen area capture windows did not appear, using the system selector");
                let (q, c) = (self.queue.clone(), self.ctx.clone());
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let res = system::capture(CaptureMode::Area, &f.opt);
                    q.lock().unwrap().push(if for_ocr { Action::OcrCaptured(res) } else { Action::Captured(res) });
                    c.request_repaint();
                });
            }
            return;
        }
        for (i, frame) in frames.iter().enumerate() {
            show_window(ctx, frozen_ui::viewport_id(i), frozen_ui::builder(i, *frame), frozen_ui::body(f.shared.clone(), i));
        }
        if !f.raised {
            f.raised = frozen_ui::raise_windows(frames.len(), f.key) == frames.len();
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(100), ViewportId::ROOT);
    }

    fn frozen_done(&mut self, out: frozen::Outcome) {
        let Some(f) = self.frozen.take() else { return };
        let st = f.shared.lock().unwrap();
        let res = match out {
            frozen::Outcome::Cancelled => Ok(None),
            frozen::Outcome::Done(d, sel) => match st.shots.get(d).and_then(|shot| frozen::crop(shot, sel).map(|img| (img, shot.px_per_point().0 as f32))) {
                Some((img, scale)) => {
                    let area = model::R { x0: sel.x as f32, y0: sel.y as f32, x1: (sel.x + sel.w) as f32, y1: (sel.y + sel.h) as f32 };
                    Ok(Some(Captured { img, scale, area: Some(area), mode: CaptureMode::Area }))
                }
                None => Err("Can't cut that area from the picture of the screen.".to_string()),
            },
        };
        self.push(if st.for_ocr { Action::OcrCaptured(res) } else { Action::Captured(res) });
    }

    /// Starts a scrolling capture, or ends the running one (the hotkey and the menu item act as Done).
    fn start_scrolling(&mut self, dir: scroll::Direction) {
        if let Some(s) = &self.scroll {
            s.finish();
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        self.scroll_dir = dir;
        let was_visible = !self.hidden;
        if was_visible {
            self.hide_window();
        }
        let (q, ctx) = (self.queue.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            if was_visible {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let res = scroll::select_area();
            q.lock().unwrap().push(Action::ScrollingArea(res));
            ctx.request_repaint();
        });
    }

    fn scrolling_area(&mut self, res: Result<Option<model::R>, String>) {
        let area = match res {
            Ok(Some(area)) => area,
            Ok(None) => {
                self.busy = false;
                return;
            }
            Err(e) => {
                self.busy = false;
                self.error(e);
                return;
            }
        };
        let Some(rect) = scroll::GrabRect::from_area(&area) else {
            self.busy = false;
            self.error("That area is too small. Select a larger area to scroll through.");
            return;
        };
        let Some(pos) = scroll::control_position(&rect.as_r(), scroll::CONTROL_SIZE, &scroll::screens()) else {
            self.busy = false;
            self.error("No room for the controls next to that area. Select a smaller area.");
            return;
        };
        // Scrolling for the user needs the Accessibility permission; ask for it the first time. Without it
        // the capture still works, the user scrolls.
        let limits = scroll::Limits::from_settings(&self.settings);
        let auto = !limits.manual && autoscroll::trusted(true);
        self.scroll = Some(scroll::Session::start(rect, pos, auto, self.scroll_dir, limits, self.queue.clone(), self.ctx.clone()));
        self.grab_escape(true);
        // The control is a new window and the app has no Dock icon: wake the UI pass that creates it
        // while the editor is hidden, and show it without taking the keyboard from the page.
        self.wake_ui();
        self.show_without_focus(scroll::CONTROL_TITLE);
        self.ctx.request_repaint();
    }

    /// Return: ends the running scrolling capture and gives Return back at once. With no capture
    /// running (a press that was already queued when the capture ended, or a double press) nothing
    /// happens: it must never open the area picker of a new capture.
    fn finish_scrolling(&mut self) {
        if let Some(s) = &self.scroll {
            s.finish();
        }
        self.release_enter();
    }

    fn release_enter(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            s.release_enter();
        }
    }

    fn scrolling_finished(&mut self, res: Result<Option<scroll::Outcome>, String>) {
        self.scroll = None;
        let up = self.scroll_dir == scroll::Direction::Up;
        self.grab_escape(false);
        match res {
            Ok(Some(o)) => {
                let (frames, lost, capped, max_height) = (o.frames, o.lost, o.capped, o.max_height);
                let size = (o.img.w, o.img.h);
                let (img, scale) = (o.img, o.scale);
                self.handle(Action::Captured(Ok(Some(Captured { img, scale, area: None, mode: CaptureMode::Area }))));
                if capped {
                    self.say(format!("Stopped at the height limit of {max_height} px. Change it in Preferences"));
                } else if frames <= 1 && lost > 0 {
                    self.say("Lost track while scrolling, so this is only the first view. Scroll slower and try again");
                } else if frames <= 1 && up {
                    self.say("The page didn't scroll up, so this is a regular screenshot. Scroll to the bottom of the page first");
                } else if frames <= 1 {
                    self.say("The page didn't scroll, so this is a regular screenshot");
                } else {
                    self.say(format!("Final image size: {} × {} px", size.0, size.1));
                }
            }
            Ok(None) => self.handle(Action::Captured(Ok(None))),
            Err(e) => self.handle(Action::Captured(Err(e))),
        }
    }

    fn scroll_ui(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.scroll else { return };
        let builder = ViewportBuilder::default()
            .with_title(scroll::CONTROL_TITLE)
            .with_inner_size(scroll::CONTROL_SIZE)
            .with_position(s.control_pos)
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_active(false)
            .with_always_on_top();
        show_window(ctx, scroll::control_id(), builder, s.control_body());
    }

    pub fn grab_escape(&mut self, on: bool) {
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            s.grab_escape(on);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = on;
    }

    pub fn open_image(&mut self, img: Image, scale: f32, fullscreen: bool) {
        self.doc = Some(Doc::new(img, scale));
        self.ed.reset_for_new_doc(fullscreen && self.settings.prefer_100);
        self.show_window();
        let (w, h) = {
            let d = self.doc.as_ref().unwrap();
            (d.base.w as f32 / scale.max(1.0), d.base.h as f32 / scale.max(1.0))
        };
        let chrome = editor::TOOLBAR_H + editor::OPTIONS_H + editor::STATUS_H;
        system::center_main_window(&self.ctx, |screen_w, screen_h| {
            sizing::editor_window((w, h), chrome, WINDOW_MARGIN, sizing::full_toolbar_width(), WINDOW_MIN_HEIGHT_RATIO, (screen_w, screen_h))
        });
    }

    /// The result of an Add Capture: the new picture joins the current one on a wider canvas.
    fn capture_added(&mut self, res: Result<Option<Captured>, String>) {
        let Some(d) = &mut self.doc else { return };
        match res {
            Ok(Some(c)) => {
                // Same screen, same scale in practice; if not, bring the new picture to the document's.
                let k = d.scale_factor / c.scale.max(0.01);
                let img = if (k - 1.0).abs() > 0.01 { render::resize(&c.img, k) } else { c.img };
                d.append_capture(&img);
                let (w, h) = (d.base.w, d.base.h);
                self.canvas_changed();
                self.show_window();
                self.say(format!("Canvas is now {w} × {h} px"));
            }
            // Cancelled: the editor was hidden for the grab, bring it back as it was.
            Ok(None) => self.show_window(),
            Err(e) => {
                self.show_window();
                self.error(e);
            }
        }
    }

    fn paste_image(&mut self) {
        if self.doc.is_none() {
            return;
        }
        match system::clipboard_image() {
            Ok(img) => {
                // The picture is converted at full size on every redraw: a huge one is brought down first.
                let img = if model::too_big_to_paste(&img) { render::resize(&img, model::paste_shrink_factor(&img)) } else { img };
                let d = self.doc.as_mut().unwrap();
                let i = d.paste_image(img);
                self.select_object(i);
            }
            Err(_) => self.error("Image overlay takes the image from the clipboard. Your clipboard doesn't seem to contain an image at this moment."),
        }
    }

    fn save_gif(&mut self) {
        let Some(d) = &self.doc else { return };
        let frames = render::gif_frames(d, &self.fonts).and_then(|(a, b)| render::encode_gif(&[a, b]));
        let bytes = match frames {
            Ok(b) => b,
            Err(e) => return self.error(e),
        };
        if !std::path::Path::new(&self.settings.folder).is_dir() {
            return self.error("Folder doesn't exist. Choose a screenshots folder in Preferences.");
        }
        let stem = settings::file_stem(&self.settings.file_name_template, &system::now_stamp());
        let path = system::unique_path(&PathBuf::from(&self.settings.folder), &stem, "gif");
        match std::fs::write(&path, bytes) {
            Ok(()) => self.say(format!("Saved to {}", path.display())),
            Err(e) => self.error(format!("Can't save: {e}")),
        }
    }

    pub fn export(&self) -> Option<Image> {
        self.doc.as_ref().map(|d| render::export(d, &self.fonts))
    }

    pub fn copy(&mut self) {
        let Some(img) = self.export() else { return };
        match system::copy_image(&img) {
            Ok(()) => {
                self.flash_tray();
                self.say("Copied to the clipboard")
            }
            Err(e) => self.error(format!("Can't copy the image: {e}")),
        }
    }

    fn save_target(&self, img: &Image) -> PathBuf {
        let ext = if self.settings.save_auto_format && system::looks_like_photo(img) { "jpg" } else { "png" };
        let stem = settings::file_stem(&self.settings.file_name_template, &system::now_stamp());
        system::unique_path(&PathBuf::from(&self.settings.folder), &stem, ext)
    }

    fn prepared_for_save(&self) -> Option<Image> {
        let img = self.export()?;
        let scale = self.doc.as_ref().map(|d| d.scale_factor).unwrap_or(1.0);
        Some(if self.settings.downscale_retina && scale > 1.0 { render::resize(&img, 1.0 / scale) } else { img })
    }

    pub fn save(&mut self) {
        let Some(img) = self.prepared_for_save() else { return };
        if !std::path::Path::new(&self.settings.folder).is_dir() {
            self.error("Folder doesn't exist. Choose a screenshots folder in Preferences.");
            return;
        }
        let path = self.save_target(&img);
        match system::save_image(&img, &path) {
            Ok(()) => {
                self.flash_tray();
                self.say(format!("Saved to {}", path.display()))
            }
            Err(e) => self.error(format!("Can't save: {e}")),
        }
    }

    pub fn save_as(&mut self) {
        let Some(img) = self.prepared_for_save() else { return };
        let suggested = self.save_target(&img);
        let name = suggested.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let picked = rfd::FileDialog::new()
            .set_directory(&self.settings.folder)
            .set_file_name(name)
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .save_file();
        if let Some(path) = picked {
            match system::save_image(&img, &path) {
                Ok(()) => {
                    self.flash_tray();
                    self.say(format!("Saved to {}", path.display()))
                }
                Err(e) => self.error(format!("Can't save: {e}")),
            }
        }
    }

    /// Opens the system print panel for the current image, with its annotations.
    pub fn print(&mut self) {
        let Some(img) = self.export() else { return };
        let scale = self.doc.as_ref().map(|d| d.scale_factor).unwrap_or(1.0);
        if let Err(e) = system::print_image(&img, scale, "ShotCraft") {
            self.error(format!("Can't print: {e}"));
        }
    }

    /// Closes every window the app shows for screenshots: the pins and the thumbnail go, and the
    /// editor hides to the menu bar. Preferences stay, and the app keeps running.
    pub fn close_all(&mut self) {
        for p in &self.pins {
            p.lock().unwrap().open = false;
        }
        self.pins.clear();
        self.thumb = None;
        self.hide_window();
    }

    pub fn pin(&mut self, img: Image, scale: f32) {
        let id = self.next_pin;
        self.next_pin += 1;
        self.pins.push(Arc::new(Mutex::new(Pin { id, img, scale, zoom: 1.0, semi: false, scroll: 0.0, screen: screen_size(&self.ctx), wheel: sizing::WheelZoom::default(), reverse_wheel: self.settings.reverse_zoom, tex: None, open: true })));
        self.wake_ui();
        self.bring_to_front("ShotCraft Pin");
    }

    pub fn pin_current(&mut self) {
        let Some(img) = self.export() else { return };
        let scale = self.doc.as_ref().map(|d| d.scale_factor).unwrap_or(1.0);
        self.pin(img, scale);
    }

    /// macOS can take many seconds to load its text model the first time, so this never runs on
    /// the UI thread.
    pub fn ocr_image(&mut self, img: &Image) {
        if self.ocr_running {
            return;
        }
        self.ocr_running = true;
        self.say("Reading text…");
        let (img, queue, ctx, language) = (img.clone(), self.queue.clone(), self.ctx.clone(), self.settings.ocr_language.clone());
        std::thread::spawn(move || {
            let result = system::recognize_in(&img, &language);
            queue.lock().unwrap().push(Action::OcrDone(result));
            ctx.request_repaint();
        });
    }

    fn ocr_done(&mut self, result: Result<system::OcrResult, String>) {
        self.ocr_running = false;
        match result {
            Ok(res) => {
                let sep = if self.settings.ocr_remove_breaks { " " } else { "\n" };
                let mut text = res.lines.join(sep);
                if !res.codes.is_empty() {
                    if !text.is_empty() {
                        text.push_str(sep);
                    }
                    text.push_str(&res.codes.join(sep));
                }
                if text.trim().is_empty() {
                    self.error("No text detected");
                } else {
                    system::copy_text(&text);
                    self.say(if self.settings.ocr_remove_breaks { "Text without line breaks copied" } else { "Text copied to the clipboard" });
                }
            }
            Err(e) => self.error(e),
        }
    }

    fn handle(&mut self, a: Action) {
        // These come from the status item menu too. They change the current image, so the editor
        // is shown to make the change visible.
        if self.hidden && self.doc.is_some() && matches!(a, Action::Undo | Action::Redo | Action::ResetCrop | Action::Rasterize | Action::SaveAs) {
            self.show_window();
        }
        match a {
            Action::Capture(m) => self.start_capture(m, false),
            Action::InstantOcr => self.start_capture(CaptureMode::Area, true),
            Action::Captured(res) => {
                self.busy = false;
                if std::mem::take(&mut self.appending) {
                    return self.capture_added(res);
                }
                match res {
                    Ok(Some(c)) => {
                        if let Some(r) = c.area {
                            self.settings.last_area = Some([r.x0, r.y0, r.x1, r.y1]);
                            self.settings.save();
                        }
                        let full = matches!(c.mode, CaptureMode::Fullscreen | CaptureMode::Delayed | CaptureMode::AllDisplays);
                        self.doc = Some(Doc::new(c.img.clone(), c.scale));
                        if self.settings.after_copy {
                            self.copy();
                        }
                        if self.settings.after_save {
                            self.save();
                        }
                        match self.settings.after_show {
                            AfterShow::Editor => self.open_image(c.img, c.scale, full),
                            AfterShow::Thumbnail => {
                                self.ed.reset_for_new_doc(false);
                                self.wake_ui();
                                self.thumb = Some(Arc::new(Mutex::new(Thumb { img: c.img, scale: c.scale, tex: None, born: Instant::now(), hovered: false, focused: false, blurred: false })));
                            }
                            AfterShow::Nothing => self.ed.reset_for_new_doc(false),
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.error(e),
                }
            }
            Action::OcrCaptured(res) => {
                self.busy = false;
                match res {
                    Ok(Some(c)) => self.ocr_image(&c.img),
                    Ok(None) => {}
                    Err(e) => self.error(e),
                }
            }
            Action::OcrDone(result) => self.ocr_done(result),
            Action::FrozenShots(start) => self.frozen_start(*start),
            Action::ScrollingCapture => self.start_scrolling(scroll::Direction::Down),
            Action::ScrollingCaptureUp => self.start_scrolling(scroll::Direction::Up),
            Action::ScrollingCancel => {
                if let Some(s) = &self.scroll {
                    s.press_escape();
                }
            }
            Action::ScrollingFinish => self.finish_scrolling(),
            Action::ScrollingArea(res) => self.scrolling_area(res),
            Action::ScrollingFinished(res) => self.scrolling_finished(res),
            Action::AddCapture => {
                if self.doc.is_some() && !self.busy {
                    self.appending = true;
                    self.start_capture(CaptureMode::Area, false);
                }
            }
            Action::Paste => self.paste_image(),
            Action::SaveGif => self.save_gif(),
            Action::OpenClipboard => match system::clipboard_image() {
                Ok(img) => {
                    let s = self.pixels_per_point();
                    self.open_image(img, s, false)
                }
                Err(e) => {
                    self.show_window();
                    self.error(e)
                }
            },
            Action::OpenFile => {
                let picked = rfd::FileDialog::new().add_filter("Images", system::OPEN_EXTENSIONS).pick_file();
                if let Some(path) = picked {
                    match system::load_image_file(&path) {
                        Ok(img) => self.open_image(img, 1.0, false),
                        Err(e) => self.error(e),
                    }
                }
            }
            Action::Show => self.show_window(),
            Action::Preferences => self.open_prefs(),
            Action::CheckForUpdates => {
                #[cfg(target_os = "macos")]
                if let Some(s) = &self.shell {
                    s.check_for_updates();
                }
            }
            Action::ToggleAutoUpdates => {
                #[cfg(target_os = "macos")]
                if let Some(s) = &self.shell {
                    s.toggle_auto_updates();
                }
            }
            Action::ToggleSnap(similar) => {
                let flag = if similar { &mut self.settings.snap_similar } else { &mut self.settings.snap_objects };
                *flag = !*flag;
                self.settings.save();
                self.sync_shell();
            }
            Action::ToggleAutostart => {
                self.settings.autostart = !self.settings.autostart;
                system::set_autostart(self.settings.autostart);
                self.settings.save();
                self.sync_shell();
            }
            Action::Quit => {
                self.quitting = true;
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Action::Copy => self.copy(),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::Pin => self.pin_current(),
            Action::Print => self.print(),
            Action::Close => self.hide_window(),
            Action::CloseAll => self.close_all(),
            Action::About => {
                self.show_window();
                self.about_open = true
            }
            Action::Shortcuts => {
                self.show_window();
                self.shortcuts_open = true
            }
            Action::PickFolder => {
                if let Some(dir) = rfd::FileDialog::new().set_directory(&self.settings.folder).pick_folder() {
                    self.settings.folder = dir.display().to_string();
                    self.settings.save();
                }
            }
            Action::PinEdit(img, scale) => self.open_image(img, scale, false),
            Action::PinCopy(img) => match system::copy_image(&img) {
                Ok(()) => self.say("Copied to the clipboard"),
                Err(e) => self.error(e),
            },
            Action::ThumbDo(act) => self.thumb_action(act),
            other => self.editor_action(other),
        }
    }

    pub fn open_prefs(&mut self) {
        let mut p = self.prefs.lock().unwrap();
        p.focus = p.open;
        p.open = true;
        drop(p);
        self.wake_ui();
        self.bring_to_front("Preferences");
    }

    /// Tells the menus what the editor allows now (an image, the keyboard, a text being typed). Run
    /// every frame, it only reaches AppKit when something changed.
    fn sync_menu_state(&mut self) {
        #[cfg(target_os = "macos")]
        if self.shell.is_some() {
            let focused = self.ctx.input_for(ViewportId::ROOT, |i| i.viewport().focused.unwrap_or(false));
            let keys = self.editor_key_state();
            let st = menu_model::MenuState { has_image: self.doc.is_some(), editor_visible: !self.hidden, editor_focused: focused, typing: keys.typing || self.ctx.egui_wants_keyboard_input(), ..keys };
            if let Some(s) = &mut self.shell {
                s.set_state(st);
            }
        }
    }

    /// The menu bar icon confirms a copy or a save with a check mark for a moment.
    fn flash_tray(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            s.flash_done();
            self.ctx.request_repaint_after(std::time::Duration::from_millis(650));
        }
    }

    pub fn sync_shell(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            s.sync(&self.settings);
        }
    }

    /// Switches the menu key equivalents and the global hotkeys off while Preferences records a
    /// shortcut, and back on afterwards.
    pub fn set_keys_suspended(&mut self, on: bool) {
        if self.keys_suspended == on {
            return;
        }
        self.keys_suspended = on;
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            let refused = s.suspend_keys(on, &self.settings);
            if !on {
                self.prefs.lock().unwrap().set_refused(refused);
            }
        }
    }

    pub fn apply_hotkeys(&mut self) -> Vec<settings::HotkeyAction> {
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            return s.apply_hotkeys(&self.settings);
        }
        vec![]
    }

    pub fn apply_window_level(&self) {
        let level = if self.settings.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
        self.ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
    }

    fn drain(&mut self) {
        let actions: Vec<Action> = std::mem::take(&mut *self.queue.lock().unwrap());
        for a in actions {
            self.handle(a);
        }
    }

    fn pins_ui(&mut self, ctx: &egui::Context) {
        self.pins.retain(|p| p.lock().unwrap().open);
        for pin in &self.pins {
            let (id, size) = {
                let mut p = pin.lock().unwrap();
                p.screen = screen_size(ctx);
                p.reverse_wheel = self.settings.reverse_zoom;
                (p.id, p.layout().window)
            };
            let builder = ViewportBuilder::default()
                .with_title("ShotCraft Pin")
                .with_inner_size([size.0, size.1])
                .with_decorations(false)
                .with_resizable(false)
                .with_transparent(true)
                .with_always_on_top();
            let (shared, queue) = (pin.clone(), self.queue.clone());
            show_window(ctx, ViewportId::from_hash_of(("pin", id)), builder, move |ui| pin_window(ui, &mut shared.lock().unwrap(), &queue));
        }
    }

    fn thumb_ui(&mut self, ctx: &egui::Context) {
        let Some(shared) = self.thumb.clone() else { return };
        let (dims, close) = {
            let th = shared.lock().unwrap();
            let idle = th.born.elapsed().as_secs_f32();
            ((th.img.w as u32, th.img.h as u32), sizing::thumb_should_close(self.settings.thumbnail_auto_close, idle, th.hovered, th.blurred))
        };
        if close {
            self.thumb = None;
            return;
        }
        let window = sizing::thumb_layout(dims, model::THUMB_WIDTH, model::THUMB_MAX_HEIGHT).window;
        let size = egui::Vec2::from(window);
        let monitor = screen_size(ctx);
        let pos = egui::Pos2::from(sizing::thumb_origin((monitor.x, monitor.y), window));
        let builder = ViewportBuilder::default()
            .with_title("ShotCraft Preview")
            .with_inner_size(size)
            .with_position(pos)
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_always_on_top();
        let queue = self.queue.clone();
        show_window(ctx, ViewportId::from_hash_of("thumb"), builder, move |ui| thumb_window(ui, &mut shared.lock().unwrap(), &queue));
        ctx.request_repaint_after_for(std::time::Duration::from_millis(250), ViewportId::ROOT);
    }

    fn thumb_action(&mut self, act: ThumbAction) {
        let Some((img, scale)) = self.thumb.as_ref().map(|t| {
            let t = t.lock().unwrap();
            (t.img.clone(), t.scale)
        }) else {
            return;
        };
        match act {
            ThumbAction::Copy => return self.copy(),
            ThumbAction::Save => return self.save(),
            ThumbAction::Edit => self.open_image(img, scale, false),
            ThumbAction::Ocr => self.ocr_image(&img),
            ThumbAction::Pin => self.pin(img, scale),
            ThumbAction::Close => {}
        }
        self.thumb = None;
    }

    fn screenshot_tick(&mut self, ui: &mut egui::Ui) {
        let Some((path, frames)) = &mut self.screenshot else { return };
        *frames += 1;
        ui.ctx().request_repaint();
        if *frames == 120 {
            ui.ctx().send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ui.input(|i| {
            i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let px: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, px) {
                if let Err(e) = buf.save(&path) {
                    eprintln!("screenshot: {e}");
                }
            }
            self.quitting = true;
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
        if *frames > 600 {
            self.quitting = true;
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

impl Pin {
    /// The picture at its zoom, then bounded to the screen: see `sizing::pin_layout`.
    fn layout(&self) -> sizing::PinLayout {
        Self::layout_of(&self.img, self.scale, self.zoom, self.screen)
    }

    fn layout_of(img: &Image, scale: f32, zoom: f32, screen: egui::Vec2) -> sizing::PinLayout {
        let content = egui::vec2(img.w as f32, img.h as f32) / scale.max(1.0) * zoom;
        sizing::pin_layout((content.x, content.y), sizing::pin_max((screen.x, screen.y)))
    }
}

fn screen_size(ctx: &egui::Context) -> egui::Vec2 {
    ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1440.0, 900.0))
}

/// Resizes a pin's window around its centre, so the picture grows and shrinks under the pointer.
fn resize_pin_window(ctx: &egui::Context, old: sizing::Size, new: sizing::Size) {
    ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(new.0, new.1)));
    if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
        let at = sizing::recentered((outer.min.x, outer.min.y), old, new);
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(at.0, at.1)));
    }
    ctx.request_repaint_of(ViewportId::ROOT);
}

/// Turns the drop shadow of the pin window under `ctx` on or off. The pin windows share a title, so
/// this one is found by its frame. (Asking eframe for another `has_shadow` would rebuild the window.)
#[cfg(target_os = "macos")]
fn set_pin_shadow(ctx: &egui::Context, shadow: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSScreen};
    let Some(outer) = ctx.input(|i| i.viewport().outer_rect) else { return };
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(primary) = NSScreen::screens(mtm).iter().next() else { return };
    let top = primary.frame().size.height;
    for w in NSApplication::sharedApplication(mtm).windows().iter().filter(|w| w.title().to_string() == "ShotCraft Pin") {
        let f = w.frame();
        let (x, y) = (f.origin.x as f32, (top - f.origin.y - f.size.height) as f32);
        if (x - outer.min.x).abs() < 2.0 && (y - outer.min.y).abs() < 2.0 {
            w.setHasShadow(shadow);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_pin_shadow(_ctx: &egui::Context, _shadow: bool) {}

fn pin_window(ui: &mut egui::Ui, pin: &mut Pin, queue: &Queue) {
    let ctx = ui.ctx().clone();
    let (img, scale) = (pin.img.clone(), pin.scale);
    let layout = pin.layout();
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let tint = if pin.semi { Color32::from_white_alpha(140) } else { Color32::WHITE };
    let mut layout = layout;
    if resp.hovered() {
        // The wheel resizes the pin, and scrolls it instead when the picture is taller than the
        // screen (Option or Command held: the other one). See `sizing::pin_wheel_action`.
        let tall = Pin::layout_of(&img, scale, 1.0, pin.screen).max_scroll() > 0.0;
        let (events, now) = ui.input(|i| (i.raw.events.clone(), i.time));
        let mut resized_from = None;
        for e in &events {
            let egui::Event::MouseWheel { unit, delta, phase, modifiers } = e else { continue };
            let precise = *unit == egui::MouseWheelUnit::Point;
            let phase = match phase {
                egui::TouchPhase::Start => sizing::WheelPhase::Start,
                egui::TouchPhase::Move => sizing::WheelPhase::Move,
                egui::TouchPhase::End | egui::TouchPhase::Cancel => sizing::WheelPhase::End,
            };
            if sizing::pin_wheel_action(tall, modifiers.alt || modifiers.command) == sizing::PinWheel::Scroll {
                pin.scroll -= if precise { delta.y } else { delta.y * 50.0 };
                pin.scroll = pin.scroll.clamp(0.0, layout.max_scroll());
                continue;
            }
            let Some(zoom_in) = pin.wheel.feed(precise, delta.y, phase, pin.reverse_wheel, now) else { continue };
            // The zoom as shown, which is lower than `pin.zoom` while the screen caps the pin.
            let natural = egui::vec2(img.w as f32, img.h as f32) / scale.max(1.0) * pin.zoom;
            let shown_pct = (pin.zoom * 100.0 * layout.content.0 / natural.x.max(1.0)).round() as u32;
            let max_w = sizing::pin_max((pin.screen.x, pin.screen.y)).0;
            let Some(next) = sizing::pin_zoom_step(shown_pct, zoom_in, layout.window.0, max_w) else { continue };
            resized_from.get_or_insert(layout);
            pin.zoom = next as f32 / 100.0;
            let new = pin.layout();
            pin.scroll = sizing::rescaled_scroll(pin.scroll, layout.content.1, new.content.1);
            layout = new;
        }
        if let Some(old) = resized_from {
            resize_pin_window(&ctx, old.window, layout.window);
        }
    }
    pin.scroll = pin.scroll.clamp(0.0, layout.max_scroll());
    let tex = pin.tex.get_or_insert_with(|| tiled::TiledTexture::new(&ctx, &format!("pin-{}", pin.id), (img.w, img.h), &img.px, tiled::Alpha::Straight, egui::TextureOptions::LINEAR));
    let content = egui::Rect::from_min_size(rect.min - egui::vec2(0.0, pin.scroll), egui::vec2(layout.content.0, layout.content.1));
    tex.paint_in(&ui.painter().with_clip_rect(rect), content, tint);
    if resp.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let mut close = false;
    if resp.hovered() {
        let bar = egui::Rect::from_min_size(rect.right_top() + egui::vec2(-86.0, 6.0), egui::vec2(80.0, 24.0));
        ui.painter().rect_filled(bar, 6.0, Color32::from_black_alpha(150));
        let mut x = bar.left() + 4.0;
        for (icon, which) in [(icons::Icon::Edit, 0), (icons::Icon::Copy, 1), (icons::Icon::Close, 2)] {
            let r = egui::Rect::from_min_size(egui::pos2(x, bar.top() + 2.0), egui::vec2(24.0, 20.0));
            let b = ui.interact(r, ui.id().with(("pinbtn", which)), egui::Sense::click());
            if b.hovered() {
                ui.painter().rect_filled(r, 4.0, Color32::from_white_alpha(40));
            }
            icons::draw(ui.painter(), r.shrink(3.0), icon, Color32::WHITE);
            if b.clicked() {
                match which {
                    0 => send(&ctx, queue, Action::PinEdit(img.clone(), scale)),
                    1 => send(&ctx, queue, Action::PinCopy(img.clone())),
                    _ => close = true,
                }
            }
            x += 25.0;
        }
    }
    resp.context_menu(|ui| {
        if ui.button("Edit").clicked() {
            send(&ctx, queue, Action::PinEdit(img.clone(), scale));
            ui.close();
        }
        if ui.button("Copy").clicked() {
            send(&ctx, queue, Action::PinCopy(img.clone()));
            ui.close();
        }
        ui.separator();
        for (label, z) in [("50%", 0.5), ("100%", 1.0), ("200%", 2.0)] {
            if ui.radio(pin.zoom == z, label).clicked() {
                pin.zoom = z;
                let window = pin.layout().window;
                resize_pin_window(&ctx, layout.window, window);
                ctx.request_repaint_of(ViewportId::ROOT);
                ui.close();
            }
        }
        if ui.checkbox(&mut pin.semi, "Semi-transparent").clicked() {
            // A see-through pin has no shadow: it is meant to be laid over something to compare.
            set_pin_shadow(&ctx, !pin.semi);
            ui.close();
        }
        ui.separator();
        if ui.button("Close").clicked() {
            close = true;
            ui.close();
        }
    });
    if resp.double_clicked() {
        send(&ctx, queue, Action::PinEdit(img.clone(), scale));
    }
    if close || ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape) || (i.modifiers.command && i.key_pressed(egui::Key::W))) {
        pin.open = false;
        ctx.request_repaint_of(ViewportId::ROOT);
    }
}

fn thumb_window(ui: &mut egui::Ui, th: &mut Thumb, queue: &Queue) {
    let ctx = ui.ctx().clone();
    // Automatic closing follows the window losing focus.
    // `born` is when the pointer was last on it (or when it opened): the automatic timer counts from there.
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
    th.blurred = sizing::lost_focus(th.focused, focused, th.blurred);
    th.focused = focused;
    let img = th.img.clone();
    let tex = th.tex.get_or_insert_with(|| tiled::TiledTexture::new(&ctx, "thumb", (img.w, img.h), &img.px, tiled::Alpha::Straight, egui::TextureOptions::LINEAR));
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    ui.painter().rect_filled(rect, 10.0, Color32::from_gray(30));
    // The picture keeps its aspect ratio and is centred; a tall one leaves empty sides.
    let image = sizing::thumb_layout((img.w as u32, img.h as u32), model::THUMB_WIDTH, model::THUMB_MAX_HEIGHT).image;
    let img_r = egui::Rect::from_center_size(rect.center(), egui::vec2(image.0, image.1));
    tex.paint_in(ui.painter(), img_r, Color32::WHITE);
    th.hovered = resp.hovered();
    if th.hovered {
        th.born = Instant::now();
        ui.painter().rect_filled(rect.shrink(sizing::THUMB_PAD), 6.0, Color32::from_black_alpha(110));
        let items = [
            (icons::Icon::Edit, "Edit", ThumbAction::Edit),
            (icons::Icon::Copy, "Copy", ThumbAction::Copy),
            (icons::Icon::Save, "Save", ThumbAction::Save),
            (icons::Icon::Ocr, "Recognize Text/QR", ThumbAction::Ocr),
            (icons::Icon::Pin, "Pin to Screen", ThumbAction::Pin),
        ];
        let bw = 34.0;
        let mut x = rect.center().x - bw * items.len() as f32 / 2.0;
        for (i, (icon, tip, act)) in items.iter().enumerate() {
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 15.0), egui::vec2(bw - 4.0, 30.0));
            let b = ui.interact(r, ui.id().with(("thumbbtn", i)), egui::Sense::click()).on_hover_text(*tip);
            ui.painter().rect_filled(r, 6.0, if b.hovered() { Color32::from_white_alpha(60) } else { Color32::from_white_alpha(25) });
            icons::draw(ui.painter(), r.shrink(6.0), *icon, Color32::WHITE);
            if b.clicked() {
                send(&ctx, queue, Action::ThumbDo(*act));
            }
            x += bw;
        }
        let cr = egui::Rect::from_min_size(rect.left_top() + egui::vec2(8.0, 8.0), egui::vec2(18.0, 18.0));
        let c = ui.interact(cr, ui.id().with("thumbclose"), egui::Sense::click());
        ui.painter().circle_filled(cr.center(), 9.0, Color32::from_black_alpha(170));
        icons::draw(ui.painter(), cr.shrink(3.0), icons::Icon::Close, Color32::WHITE);
        if c.clicked() {
            send(&ctx, queue, Action::ThumbDo(ThumbAction::Close));
        }
    }
    if resp.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        send(&ctx, queue, Action::ThumbDo(ThumbAction::Edit));
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        #[cfg(target_os = "macos")]
        if let Some(s) = &mut self.shell {
            s.refresh_updates();
            if let Some(again) = s.tick_flash() {
                self.ctx.request_repaint_after(again);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.started {
            self.started = true;
            #[cfg(target_os = "macos")]
            if self.screenshot.is_none() {
                self.shell = Some(shell::Shell::start(self.queue.clone(), ctx.clone(), &self.settings));
                // Warm up: load the text model now, not on the first real OCR.
                std::thread::spawn(|| {
                    let _ = system::recognize(&Image { w: 64, h: 64, px: Arc::new(vec![255; 64 * 64 * 4]) });
                });
            }
            self.apply_window_level();
            if self.hidden {
                // eframe shows the window once it has painted a first frame. Keep it transparent
                // until the next one, then hide it: nothing appears on screen.
                app_mode::set_editor_alpha(0.0);
                self.startup_hide = true;
                self.woke_at = Instant::now();
            }
            if let Some(path) = self.open_at_start.take() {
                match system::load_image_file(&path) {
                    Ok(img) => self.open_image(img, 2.0, false),
                    Err(e) => self.error(e),
                }
            }
            // Test hook: open a tool so --screenshot can capture its options bar.
            if let Some(t) = self.screenshot.as_ref().and_then(|_| std::env::var("SHOTCRAFT_TOOL").ok()).and_then(|k| k.chars().next()).and_then(Tool::from_key) {
                self.push(Action::Tool(t));
            }
        }
        self.drain();
        self.sync_menu_state();
        if ui.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window();
        }
        if let Some(tab) = self.screenshot.as_ref().and_then(|_| std::env::var("SHOTCRAFT_PREFS_TAB").ok()) {
            // Test hook: draw a Preferences tab in the main window so --screenshot can capture it.
            let mut p = self.prefs.lock().unwrap();
            p.tab = tab.parse().unwrap_or(0);
            p.body(ui);
        } else {
            self.editor_ui(ui);
        }
        // The editor may have started typing or made a selection in this frame: the menu keys must
        // know before the next key arrives, not a frame later.
        self.sync_menu_state();
        self.pins_ui(&ctx);
        self.thumb_ui(&ctx);
        self.scroll_ui(&ctx);
        self.frozen_ui(&ctx);
        self.prefs_ui(&ctx);
        self.settle_windows();
        self.screenshot_tick(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    #[cfg(target_os = "macos")]
    if args.iter().any(|a| a == "--check-updater") {
        match updater::check() {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("Sparkle did not load: {e}");
                std::process::exit(1);
            }
        }
    }
    // No launch shows a window unless it is asked to render or open something; the login item's
    // `--background` flag is accepted and changes nothing.
    let launch = app_mode::Launch::parse(&args);
    let show_at_start = launch.shows_window();
    let app_mode::Launch { screenshot, open } = launch;
    // Test hook: size the main window like Preferences so --screenshot shows its real layout.
    let (size, min) = if std::env::var_os("SHOTCRAFT_PREFS_TAB").is_some() { (prefs::WINDOW, prefs::WINDOW) } else { ([1180.0, 760.0], [editor::MIN_W, editor::MIN_H]) };
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/window-icon.png")).expect("window icon"))
            .with_title(EDITOR_TITLE)
            .with_inner_size(size)
            .with_min_inner_size(min)
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false)
            .with_transparent(true),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    let options = eframe::NativeOptions { event_loop_builder: Some(app_mode::event_loop_hook(show_at_start)), ..options };
    eframe::run_native("ShotCraft", options, Box::new(move |cc| Ok(Box::new(App::new(cc, screenshot, show_at_start, open)))))
}

#[cfg(test)]
mod return_key_tests {
    use super::*;

    /// Return is a global hotkey for the length of a scrolling capture. A press that is processed
    /// after the capture ended (it was queued before, or Return was pressed twice) must not open
    /// the area picker of a new capture.
    #[test]
    fn return_without_a_running_capture_starts_nothing() {
        let mut app = App::headless(egui::Context::default());
        assert!(app.scroll.is_none());
        app.handle(Action::ScrollingFinish);
        assert!(!app.busy, "a new scrolling capture was started");
        assert!(app.scroll.is_none());
    }
}

#[cfg(test)]
mod glyph_tests {
    use eframe::egui::{self, FontFamily, FontId};

    /// Every symbol the UI prints that DM Sans lacks: modifier keys, arrows, Esc, Return, Delete, Tab.
    const SYMBOLS: &str = "⌘⇧⌥⌃←↑→↓⎋⏎⌫⌦⇥";
    /// The other special characters of the UI text.
    const PUNCTUATION: &str = "…×—•−·°";

    #[test]
    fn the_ui_fonts_cover_the_key_symbols_so_they_never_render_as_boxes() {
        let ctx = egui::Context::default();
        super::setup_fonts(&ctx);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        for family in [FontFamily::Proportional, FontFamily::Name("medium".into())] {
            for c in SYMBOLS.chars().chain(PUNCTUATION.chars()) {
                let ok = ctx.fonts_mut(|f| f.has_glyph(&FontId::new(13.0, family.clone()), c));
                assert!(ok, "{c} (U+{:04X}) is missing from {family:?}", c as u32);
            }
        }
    }

    /// The symbols must come from the bundled subsets, not from a system font that may be absent.
    #[test]
    fn the_symbol_subsets_are_bundled_and_cover_the_key_symbols_alone() {
        use ab_glyph::{Font, FontRef};
        let subsets: [&[u8]; 3] = [
            include_bytes!("../assets/fonts/NotoSymbols2-Subset.ttf"),
            include_bytes!("../assets/fonts/NotoSymbols-Subset.ttf"),
            include_bytes!("../assets/fonts/NotoMath-Subset.ttf"),
        ];
        let fonts: Vec<FontRef> = subsets.iter().map(|b| FontRef::try_from_slice(b).expect("a valid font")).collect();
        for c in SYMBOLS.chars() {
            assert!(fonts.iter().any(|f| f.glyph_id(c).0 != 0), "{c} is in no bundled subset");
        }
        assert!(subsets.iter().map(|b| b.len()).sum::<usize>() < 20_000, "the subsets stay tiny");
    }
}
