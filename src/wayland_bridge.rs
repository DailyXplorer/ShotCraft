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

/// Milliseconds on a monotonic clock, for `stalled`.
pub fn now_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// When the editor's last UI pass started (`now_ms`).
static LAST_PASS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A UI pass starts (`App::ui`).
pub fn note_pass() {
    LAST_PASS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

pub fn last_pass_ms() -> u64 {
    LAST_PASS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the UI is stalled: an action was queued at `queued_ms`, `waited_ms` have passed, and
/// no UI pass has started since it was queued (`last_pass_ms`).
pub fn stalled(queued_ms: u64, last_pass_ms: u64, waited_ms: u64, limit_ms: u64) -> bool {
    waited_ms >= limit_ms && last_pass_ms < queued_ms
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_queued_action_with_no_pass_since_is_a_stall_once_the_wait_is_over() {
        assert!(super::stalled(1_000, 900, 400, 400), "no pass since it was queued");
        assert!(!super::stalled(1_000, 1_050, 400, 400), "a pass ran after it was queued");
        assert!(!super::stalled(1_000, 900, 100, 400), "too soon to tell");
    }

    #[test]
    fn the_icon_is_premultiplied_argb_and_scaled_to_its_size_in_points() {
        assert_eq!(super::argb_premultiplied(&[255, 0, 0, 255, 200, 100, 50, 128, 9, 9, 9, 0]), vec![0, 0, 255, 255, 25, 50, 100, 128, 0, 0, 0, 0]);
        assert_eq!((super::icon_scale(320, 160.0), super::icon_scale(160, 160.0), super::icon_scale(10, 160.0), super::icon_scale(2000, 10.0)), (2, 1, 1, 4));
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
