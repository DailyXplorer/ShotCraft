//! Linux, Wayland: ShotCraft's own requests on the connection winit opened (the same `wl_display`),
//! for what winit 0.30 doesn't offer:
//!
//! * bringing the editor back with xdg-activation. GNOME sends a minimized window no frame
//!   callbacks, winit waits for one before every redraw, and ShotCraft's actions (hotkeys, links,
//!   the tray) only run in a UI pass, so a minimized editor stopped ShotCraft after one more pass.
//!   A Wayland app can't un-minimize itself, but it may activate its window with a token the
//!   desktop gave for a user action: GNOME's GlobalShortcuts portal sends one with every press.
//!   When a hotkey's action hasn't run soon after, the editor is activated with that token.
//! * drag-out: the drag of the picture's file from the editor into other apps (`start_drag`).
//!
//! Our objects live on an event queue of our own, dispatched by one thread, so nothing here waits
//! for winit's loop (which, for a minimized window, may not run).

use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat::{self, Capability, WlSeat};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{self, ZxdgOutputV1};
use wayland_client::protocol::wl_output::WlOutput;

/// What the dispatching thread and the UI thread share.
#[derive(Default)]
struct Shared {
    /// The serial of the last pointer button press: the desktop only starts a drag for the press
    /// that is being held.
    press_serial: Option<u32>,
    /// The drag that is running: its file (offered as `text/uri-list` and as the picture).
    drag: Option<Drag>,
}

struct Drag {
    path: PathBuf,
    dropped: bool,
    /// The picture that follows the pointer; destroyed when the drag ends.
    icon: Option<Icon>,
}

struct Icon {
    surface: WlSurface,
    buffer: WlBuffer,
    pool: WlShmPool,
}

impl Icon {
    fn destroy(self) {
        self.surface.destroy();
        self.buffer.destroy();
        self.pool.destroy();
    }
}

struct Bridge {
    conn: Connection,
    qh: QueueHandle<State>,
    activation: XdgActivationV1,
    editor: WlSurface,
    manager: Option<WlDataDeviceManager>,
    device: Option<WlDataDevice>,
    /// For the drag's picture (without them the drag has none).
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    shared: Arc<Mutex<Shared>>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

struct State {
    shared: Arc<Mutex<Shared>>,
    pointer_seen: bool,
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn event(_: &mut Self, _: &XdgActivationV1, _: <XdgActivationV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlSeat, ()> for State {
    fn event(state: &mut Self, seat: &WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(Capability::Pointer) && !state.pointer_seen {
                state.pointer_seen = true;
                seat.get_pointer(qh, ());
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(state: &mut Self, _: &WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_pointer::Event::Button { serial, state: WEnum::Value(wl_pointer::ButtonState::Pressed), .. } = event {
            lock(&state.shared).press_serial = Some(serial);
        }
    }
}

impl Dispatch<WlDataDeviceManager, ()> for State {
    fn event(_: &mut Self, _: &WlDataDeviceManager, _: <WlDataDeviceManager as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(_: &mut Self, _: &WlDataDevice, _: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}

    // The device announces selections and drags into our surface with new offer objects.
    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(_: &mut Self, _: &WlCompositor, _: <WlCompositor as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlShm, ()> for State {
    fn event(_: &mut Self, _: &WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlShmPool, ()> for State {
    fn event(_: &mut Self, _: &WlShmPool, _: <WlShmPool as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(_: &mut Self, _: &WlBuffer, _: <WlBuffer as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlSurface, ()> for State {
    fn event(_: &mut Self, _: &WlSurface, _: <WlSurface as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlDataOffer, ()> for State {
    fn event(_: &mut Self, _: &WlDataOffer, _: <WlDataOffer as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlDataSource, ()> for State {
    fn event(state: &mut Self, source: &WlDataSource, event: wl_data_source::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            // A target asks for the data: write it to the pipe it gave us and close it.
            wl_data_source::Event::Send { mime_type, fd } => {
                let path = lock(&state.shared).drag.as_ref().map(|d| d.path.clone());
                if let Some(path) = path {
                    std::thread::spawn(move || {
                        let _ = answer(&path, &mime_type, fd);
                    });
                }
            }
            wl_data_source::Event::DndDropPerformed => {
                if let Some(d) = lock(&state.shared).drag.as_mut() {
                    d.dropped = true;
                }
            }
            // The drop was taken, or the drag was cancelled: the file may go (later, as for a drop).
            wl_data_source::Event::DndFinished | wl_data_source::Event::Cancelled => {
                if let Some(d) = lock(&state.shared).drag.take() {
                    crate::dragout::finish(&d.path, d.dropped);
                    if let Some(icon) = d.icon {
                        icon.destroy();
                    }
                }
                source.destroy();
            }
            _ => {}
        }
    }
}

/// What a target gets for a mime type: the file's link, or the picture itself.
pub fn payload(path: &Path, mime: &str) -> Option<Vec<u8>> {
    match mime {
        "text/uri-list" => Some(format!("{}\r\n", crate::dragout::file_uri(path)).into_bytes()),
        "image/png" => std::fs::read(path).ok(),
        _ => None,
    }
}

fn answer(path: &Path, mime: &str, fd: OwnedFd) -> std::io::Result<()> {
    let Some(bytes) = payload(path, mime) else { return Ok(()) };
    let mut out = std::fs::File::from(fd);
    out.write_all(&bytes)
}

/// Attaches to winit's Wayland connection and finds the editor's surface. Call once, with the
/// editor's creation context; an error outside Wayland.
pub fn attach(cc: &eframe::CreationContext<'_>) -> Result<(), String> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    let display = cc.display_handle().map_err(|e| e.to_string())?.as_raw();
    let window = cc.window_handle().map_err(|e| e.to_string())?.as_raw();
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) = (display, window) else {
        return Err("not a Wayland window".into());
    };
    // SAFETY: winit's `wl_display`, which lives until the event loop ends, and with it ShotCraft.
    let backend = unsafe { wayland_client::backend::Backend::from_foreign_display(display.display.as_ptr().cast()) };
    let conn = Connection::from_backend(backend);
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let shared: Arc<Mutex<Shared>> = Arc::default();
    let mut state = State { shared: shared.clone(), pointer_seen: false };
    let activation: XdgActivationV1 = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("no xdg-activation ({e})"))?;
    // SAFETY: the editor's `wl_surface`, created by winit for the root window, which lives as long
    // as the app.
    let id = unsafe { wayland_client::backend::ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr().cast()) }.map_err(|e| e.to_string())?;
    let editor = WlSurface::from_id(&conn, id).map_err(|e| e.to_string())?;
    // Drag-out needs a seat (for the press serial) and the data-device manager; without them the
    // rest still works.
    let seat: Option<WlSeat> = globals.bind(&qh, 1..=5, ()).ok();
    let manager: Option<WlDataDeviceManager> = globals.bind(&qh, 1..=3, ()).ok();
    let device = seat.as_ref().zip(manager.as_ref()).map(|(s, m)| m.get_data_device(s, &qh, ()));
    let compositor: Option<WlCompositor> = globals.bind(&qh, 1..=4, ()).ok();
    let shm: Option<WlShm> = globals.bind(&qh, 1..=1, ()).ok();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    let _ = POKE.set(cc.egui_ctx.clone());
    let reader = conn.clone();
    BRIDGE.set(Bridge { conn, qh, activation, editor, manager, device, compositor, shm, shared }).map_err(|_| "attached twice".to_string())?;
    std::thread::spawn(move || {
        while dispatch_round(&reader, &mut queue, &mut state).is_ok() {}
    });
    Ok(())
}

/// One round of our queue's dispatching: what is pending, then a wait of at most 50 ms for more.
/// winit reads the same socket and may queue events for us before our wait starts; the short wait
/// means such an event is never left sitting.
fn dispatch_round(conn: &Connection, queue: &mut wayland_client::EventQueue<State>, state: &mut State) -> Result<(), String> {
    queue.dispatch_pending(state).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    if let Some(guard) = conn.prepare_read() {
        let mut fd = libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&guard.connection_fd()), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd, and a count of one.
        let ready = unsafe { libc::poll(&mut fd, 1, 50) };
        if ready > 0 {
            guard.read().map_err(|e| e.to_string())?;
        }
    }
    queue.dispatch_pending(state).map_err(|e| e.to_string())?;
    Ok(())
}

/// Activates the editor with `token` (from the desktop, for a user action). False when there is
/// no bridge (not Wayland, or attaching failed) or the request couldn't be sent.
pub fn activate_editor(token: &str) -> bool {
    let Some(b) = BRIDGE.get() else { return false };
    b.activation.activate(token.to_string(), &b.editor);
    b.conn.flush().is_ok()
}

/// Starts the drag of the picture's file from the editor, for the button press the user is
/// holding. The drag runs in the desktop; when it ends the file is cleaned up (`dragout::finish`).
pub fn start_drag(path: &Path, preview_png: &[u8], preview: (f32, f32)) -> Result<(), String> {
    let b = BRIDGE.get().ok_or("ShotCraft isn't attached to the desktop's connection")?;
    let (Some(manager), Some(device)) = (&b.manager, &b.device) else { return Err("the desktop has no drag and drop for apps".into()) };
    let serial = {
        let mut sh = lock(&b.shared);
        if sh.drag.is_some() {
            return Err("a drag is already running".into());
        }
        let serial = sh.press_serial.ok_or("no mouse press to start the drag from")?;
        sh.drag = Some(Drag { path: path.to_path_buf(), dropped: false, icon: None });
        serial
    };
    // The picture that follows the pointer, centred on it; the drag goes without one if it can't be made.
    let icon = make_icon(b, preview_png, preview);
    let source = manager.create_data_source(&b.qh, ());
    source.offer("text/uri-list".to_string());
    source.offer("image/png".to_string());
    source.set_actions(DndAction::Copy);
    device.start_drag(Some(&source), &b.editor, icon.as_ref().map(|i| &i.surface), serial);
    match lock(&b.shared).drag.as_mut() {
        Some(d) => d.icon = icon,
        // The drag already ended (the dispatching thread was quicker): nothing will destroy it later.
        None => {
            if let Some(icon) = icon {
                icon.destroy();
            }
        }
    }
    b.conn.flush().map_err(|e| e.to_string())
}

/// The drag's picture: the small preview as a shared-memory buffer on a surface of its own.
fn make_icon(b: &Bridge, png: &[u8], preview: (f32, f32)) -> Option<Icon> {
    use std::os::fd::{AsFd, FromRawFd};
    let (compositor, shm) = (b.compositor.as_ref()?, b.shm.as_ref()?);
    let rgba = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let pixels = argb_premultiplied(rgba.as_raw());
    // SAFETY: `memfd_create` returns a new file descriptor that nothing else owns, or -1, which is checked.
    let fd = unsafe {
        let raw = libc::memfd_create(c"shotcraft-drag-icon".as_ptr(), libc::MFD_CLOEXEC);
        if raw < 0 {
            return None;
        }
        OwnedFd::from_raw_fd(raw)
    };
    std::fs::File::from(fd.try_clone().ok()?).write_all(&pixels).ok()?;
    let pool = shm.create_pool(fd.as_fd(), i32::try_from(pixels.len()).ok()?, &b.qh, ());
    let buffer = pool.create_buffer(0, w as i32, h as i32, (w * 4) as i32, wl_shm::Format::Argb8888, &b.qh, ());
    let surface = compositor.create_surface(&b.qh, ());
    let scale = icon_scale(w, preview.0);
    surface.set_buffer_scale(scale);
    // Moving the picture by minus half its size puts the pointer in its middle.
    surface.attach(Some(&buffer), -((w as i32 / scale) / 2), -((h as i32 / scale) / 2));
    surface.commit();
    Some(Icon { surface, buffer, pool })
}

/// Straight RGBA as the premultiplied ARGB8888 a Wayland buffer wants (little-endian: B, G, R, A).
pub fn argb_premultiplied(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let a = u32::from(p[3]);
            let m = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
            [m(p[2]), m(p[1]), m(p[0]), p[3]]
        })
        .collect()
}

/// How many buffer pixels make a point: the preview picture is made at twice its size on screen.
pub fn icon_scale(picture_px: u32, shown_points: f32) -> i32 {
    ((picture_px as f32 / shown_points.max(1.0)).round() as i32).clamp(1, 4)
}

/// How long an action may wait for a UI pass before the editor is brought back.
pub const STALL_MS: u64 = 400;

/// A token that came ahead of the event it belongs to (a second launch for a link sends its
/// `XDG_ACTIVATION_TOKEN` just before the link), and when.
static OFFERED: Mutex<Option<(String, u64)>> = Mutex::new(None);

/// Remembers a token for the event that follows it.
pub fn offer_token(token: &str) {
    *OFFERED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((token.to_string(), now_ms()));
}

/// The token offered a moment ago, once.
pub fn take_offered_token() -> Option<String> {
    let (token, at) = OFFERED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()?;
    (now_ms().saturating_sub(at) < 5_000).then_some(token)
}

/// Milliseconds on a monotonic clock.
fn now_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// The capture crosshair is on screen (set by the picker while it lives).
pub static PICKER_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The editor's context, to poke its UI (set when the bridge attaches).
static POKE: OnceLock<eframe::egui::Context> = OnceLock::new();

/// UI passes started so far (`note_pass`).
static PASSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A UI pass starts (`App::ui`).
pub fn note_pass() {
    PASSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a repaint request got no UI pass within `STALL_MS`: a minimized editor gets no frame
/// callbacks, so nothing runs, however much waits in the queue. Asking is the only way to tell
/// that from an editor that is just idle, which answers in milliseconds.
fn ui_is_stalled() -> bool {
    let Some(ctx) = POKE.get() else { return false };
    let before = PASSES.load(std::sync::atomic::Ordering::Relaxed);
    ctx.request_repaint();
    std::thread::sleep(std::time::Duration::from_millis(STALL_MS));
    no_pass_since(before, PASSES.load(std::sync::atomic::Ordering::Relaxed))
}

/// The count of passes didn't move.
pub fn no_pass_since(before: u64, after: u64) -> bool {
    after == before
}

/// How many times, one second apart, an event is followed after it was queued: the pass that
/// runs it may be the last one (a capture starts, and its result then needs another).
const FOLLOW_ROUNDS: usize = 10;

/// The editor may be poked (and rescued) only while no crosshair is up.
pub fn may_poke(picker_up: bool) -> bool {
    !picker_up
}

/// Waits for the UI to stall, within `FOLLOW_ROUNDS` seconds. True when it did.
fn wait_for_stall() -> bool {
    for _ in 0..FOLLOW_ROUNDS {
        // The crosshair is up: the editor is hidden on purpose, so its silence says nothing, and
        // activating it now would put it among the full-screen crosshair windows.
        if !may_poke(PICKER_UP.load(std::sync::atomic::Ordering::SeqCst)) {
            std::thread::sleep(std::time::Duration::from_millis(600));
            continue;
        }
        if ui_is_stalled() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    false
}

/// An action was just queued for the UI. A minimized editor gets no frame callbacks, so no UI pass
/// would run it (or what it leads to): when the UI turns out to be stalled, the editor is
/// activated with `token` (the desktop's, for the user action that caused this), which brings it
/// back. An editor that isn't minimized answers every poke and is left alone.
pub fn rescue_if_stalled(token: String) {
    std::thread::spawn(move || {
        if wait_for_stall() {
            activate_editor(&token);
        }
    });
}

/// A monitor as the desktop lays it out, in logical (scaled) pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalOutput {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Collects the monitors' answers.
#[derive(Default)]
struct OutputState {
    outputs: Vec<(Option<(i32, i32)>, Option<(i32, i32)>)>,
}

impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, GlobalListContents> for OutputState {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlOutput, usize> for OutputState {
    fn event(_: &mut Self, _: &WlOutput, _: <WlOutput as Proxy>::Event, _: &usize, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZxdgOutputManagerV1, ()> for OutputState {
    fn event(_: &mut Self, _: &ZxdgOutputManagerV1, _: <ZxdgOutputManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZxdgOutputV1, usize> for OutputState {
    fn event(state: &mut Self, _: &ZxdgOutputV1, event: zxdg_output_v1::Event, index: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(slot) = state.outputs.get_mut(*index) else { return };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => slot.0 = Some((x, y)),
            zxdg_output_v1::Event::LogicalSize { width, height } => slot.1 = Some((width, height)),
            _ => {}
        }
    }
}

/// The monitors' logical positions and sizes, in the order the desktop announced them (which is
/// the order winit lists its monitors in, so index `i` here is `with_monitor(i)` there). Asked
/// afresh each time, on a queue of its own. Empty when the desktop doesn't offer `xdg-output`.
pub fn logical_outputs() -> Vec<LogicalOutput> {
    if BRIDGE.get().is_none() {
        return vec![];
    }
    // A connection of its own: the monitors bound here are announced to the windows of a connection
    // that holds them (wl_surface.enter), and on the editor's connection winit panics on an object
    // it didn't make. On this one nobody else is listening.
    let Ok(conn) = Connection::connect_to_env() else { return vec![] };
    let Ok((globals, mut queue)) = registry_queue_init::<OutputState>(&conn) else { return vec![] };
    let qh = queue.handle();
    let Ok(manager) = globals.bind::<ZxdgOutputManagerV1, _, _>(&qh, 1..=3, ()) else { return vec![] };
    let announced: Vec<(u32, u32)> = globals.contents().with_list(|list| list.iter().filter(|g| g.interface == "wl_output").map(|g| (g.name, g.version)).collect());
    let mut state = OutputState { outputs: vec![(None, None); announced.len()] };
    let mut keep = vec![];
    for (index, (name, version)) in announced.into_iter().enumerate() {
        let output = globals.registry().bind::<WlOutput, _, _>(name, version.min(3), &qh, index);
        keep.push((manager.get_xdg_output(&output, &qh, index), output));
    }
    let asked = queue.roundtrip(&mut state).is_ok() && queue.roundtrip(&mut state).is_ok();
    // Release what was bound, at once and whatever the answer was. The desktop names a monitor to
    // a window by the monitor object a client bound; a copy of ours that outlived this call would
    // reach winit as an object it knows nothing about, and winit panics on it.
    for (xdg, output) in keep {
        xdg.destroy();
        output.release();
    }
    let _ = conn.flush();
    if !asked {
        return vec![];
    }
    state
        .outputs
        .into_iter()
        .filter_map(|(pos, size)| {
            let ((x, y), (w, h)) = (pos?, size?);
            (w > 0 && h > 0).then_some(LogicalOutput { x, y, w: w as u32, h: h as u32 })
        })
        .collect()
}

/// A notification is on screen asking for a click (one at a time).
static ASKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Like `rescue_if_stalled`, for an event with no token (a link from a launcher that passes
/// none, such as Firefox as a snap): when the editor is stalled, a notification asks for a click.
/// The desktop gives a click on a notification an activation token (GNOME sends the
/// `ActivationToken` signal with it), and that brings the editor back.
pub fn rescue_with_notification() {
    if BRIDGE.get().is_none() {
        return;
    }
    std::thread::spawn(move || {
        if !wait_for_stall() || ASKING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if let Err(e) = async_io::block_on(ask_to_restore()) {
            eprintln!("notification: {e}");
        }
        ASKING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// Shows the notification and waits (up to 30 s) for its click's token, then activates the editor.
async fn ask_to_restore() -> Result<(), String> {
    use ashpd::zbus;
    use std::collections::HashMap;
    let err = |e: zbus::Error| e.to_string();
    let conn = zbus::Connection::session().await.map_err(err)?;
    let proxy = zbus::Proxy::new(&conn, "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications").await.map_err(err)?;
    let mut tokens = proxy.receive_signal("ActivationToken").await.map_err(err)?;
    let hints: HashMap<&str, zbus::zvariant::Value> = HashMap::from([("desktop-entry", "shotcraft".into())]);
    let reply = proxy
        .call_method("Notify", &("ShotCraft", 0u32, "shotcraft", "ShotCraft is waiting", "Click to bring its window back.", vec!["default", "Show"], hints, 30_000i32))
        .await
        .map_err(err)?;
    let id: u32 = reply.body().deserialize().map_err(|e| e.to_string())?;
    let wait = async {
        while let Some(msg) = futures_lite::StreamExt::next(&mut tokens).await {
            if let Ok((nid, token)) = msg.body().deserialize::<(u32, String)>() {
                if nid == id {
                    return Some(token);
                }
            }
        }
        None
    };
    let timeout = async {
        async_io::Timer::after(std::time::Duration::from_secs(30)).await;
        None
    };
    if let Some(token) = futures_lite::future::or(wait, timeout).await {
        activate_editor(&token);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_editor_is_left_alone_while_the_crosshair_is_up() {
        assert!(!super::may_poke(true), "hidden on purpose: its silence is not a stall");
        assert!(super::may_poke(false));
    }

    #[test]
    fn a_poke_that_got_no_pass_is_a_stall() {
        assert!(super::no_pass_since(7, 7), "the UI never answered");
        assert!(!super::no_pass_since(7, 8), "a pass ran");
    }

    #[test]
    fn an_offered_token_is_given_once_to_the_event_that_follows() {
        assert_eq!(super::take_offered_token(), None, "nothing offered");
        super::offer_token("token-1");
        assert_eq!(super::take_offered_token().as_deref(), Some("token-1"));
        assert_eq!(super::take_offered_token(), None, "only once");
    }

    #[test]
    fn a_target_gets_a_file_link_or_the_picture_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("shotcraft-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a b.png");
        std::fs::write(&path, b"PNGDATA").unwrap();
        let uri = super::payload(&path, "text/uri-list").expect("uri list");
        assert!(uri.starts_with(b"file:///") && uri.ends_with(b"a%20b.png\r\n"), "{}", String::from_utf8_lossy(&uri));
        assert_eq!(super::payload(&path, "image/png").as_deref(), Some(&b"PNGDATA"[..]));
        assert_eq!(super::payload(&path, "text/plain"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
