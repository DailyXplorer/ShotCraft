//! Linux, X11: drag-out of the picture's file with the XDND protocol (the X drag and drop that GTK,
//! Qt, Firefox, file managers and chat apps speak), as the source.
//!
//! The drag runs on a connection of its own while the user holds the mouse button, with no pointer
//! grab (the button press belongs to the editor's window until the button goes up, and another
//! client can't grab over it): every tick the pointer and the button are asked from the server, the
//! window under it that speaks XDND gets Enter / Position / Leave, and on release Drop, after which
//! it asks for the data (`XdndSelection`, `text/uri-list`) and says Finished. Esc cancels. The call
//! returns when the drag is over, as the Windows one does.

use std::path::Path;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, KeyButMask, PropMode, SelectionNotifyEvent, Window, WindowClass, CLIENT_MESSAGE_EVENT, SELECTION_NOTIFY_EVENT,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

/// The XDND version we speak. A target that speaks less is spoken to in its version.
const VERSION: u32 = 5;
/// How often the pointer is looked at.
const TICK: Duration = Duration::from_millis(15);
/// The longest a drag may take, and how long a target gets to take the drop.
const LONGEST: Duration = Duration::from_secs(120);
const DROP_WAIT: Duration = Duration::from_secs(4);

struct Atoms {
    aware: Atom,
    enter: Atom,
    position: Atom,
    status: Atom,
    leave: Atom,
    drop: Atom,
    finished: Atom,
    selection: Atom,
    copy: Atom,
    uri_list: Atom,
    targets: Atom,
}

fn atoms(conn: &RustConnection) -> Result<Atoms, String> {
    let get = |name: &str| -> Result<Atom, String> { conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map(|r| r.atom).map_err(|e| e.to_string()) };
    Ok(Atoms {
        aware: get("XdndAware")?,
        enter: get("XdndEnter")?,
        position: get("XdndPosition")?,
        status: get("XdndStatus")?,
        leave: get("XdndLeave")?,
        drop: get("XdndDrop")?,
        finished: get("XdndFinished")?,
        selection: get("XdndSelection")?,
        copy: get("XdndActionCopy")?,
        uri_list: get("text/uri-list")?,
        targets: get("TARGETS")?,
    })
}

/// The `file://` link a target is given, as a `text/uri-list` line.
fn uri_list(path: &Path) -> Vec<u8> {
    format!("{}\r\n", crate::dragout::file_uri(path)).into_bytes()
}

/// A target's reply to a position: the window takes the drop here.
pub fn status_accepts(flags: u32) -> bool {
    flags & 1 != 0
}

/// The version to speak with a target that says it speaks `theirs`.
pub fn negotiated(theirs: u32) -> u32 {
    theirs.min(VERSION)
}

/// The packed `x << 16 | y` of an XdndPosition message (negative parts are cut to zero).
pub fn pack_position(x: i32, y: i32) -> u32 {
    ((x.clamp(0, 0xFFFF) as u32) << 16) | y.clamp(0, 0xFFFF) as u32
}

fn message(window: Window, kind: Atom, data: [u32; 5]) -> ClientMessageEvent {
    ClientMessageEvent { response_type: CLIENT_MESSAGE_EVENT, format: 32, sequence: 0, window, type_: kind, data: data.into() }
}

fn send(conn: &RustConnection, to: Window, event: ClientMessageEvent) {
    let _ = conn.send_event(false, to, EventMask::NO_EVENT, event);
    let _ = conn.flush();
}

/// The topmost window under the pointer that speaks XDND (itself or an ancestor of the deepest
/// window there), and its version.
fn target_under(conn: &RustConnection, root: Window, at: &Atoms, x: i32, y: i32) -> Option<(Window, u32)> {
    let mut chain = vec![];
    let mut window = root;
    for _ in 0..32 {
        // The child of `window` that holds the point (given in the root's coordinates).
        let r = conn.translate_coordinates(root, window, x as i16, y as i16).ok()?.reply().ok()?;
        if r.child == NONE {
            break;
        }
        chain.push(r.child);
        window = r.child;
    }
    for &w in chain.iter().rev() {
        let prop = conn.get_property(false, w, at.aware, AtomEnum::ATOM, 0, 1).ok()?.reply().ok()?;
        if let Some(v) = prop.value32().and_then(|mut v| v.next()) {
            return Some((w, negotiated(v)));
        }
    }
    None
}

/// The key code of Escape, for telling the user cancelled.
fn escape_keycode(conn: &RustConnection) -> Option<u8> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
    let per = usize::from(map.keysyms_per_keycode);
    map.keysyms.chunks(per.max(1)).position(|c| c.contains(&0xFF1B)).map(|i| min + i as u8)
}

/// Drags `path` from the pointer's current place, on the X display named `display` (`None`: the
/// environment's), while the primary button is held. Returns whether a target took the drop.
pub fn drag_on(display: Option<&str>, path: &Path) -> Result<bool, String> {
    let (conn, screen_num) = x11rb::connect(display).map_err(|e| format!("no X11 display ({e})"))?;
    let root = conn.setup().roots.get(screen_num).ok_or("no X11 screen")?.root;
    let at = atoms(&conn)?;
    // The source window: a small unseen one, to own the selection and receive the replies.
    let source = conn.generate_id().map_err(|e| e.to_string())?;
    conn.create_window(COPY_DEPTH_FROM_PARENT, source, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new())
        .map_err(|e| e.to_string())?;
    conn.set_selection_owner(source, at.selection, CURRENT_TIME).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    let escape = escape_keycode(&conn);
    let data = uri_list(path);

    let started = Instant::now();
    let mut current: Option<(Window, u32)> = None;
    let mut accepted = false;
    let mut last_sent = (-1, -1);
    let mut last_position = Instant::now() - TICK;
    let mut dropped = false;
    let outcome = loop {
        if started.elapsed() > LONGEST {
            break false;
        }
        let ptr = conn.query_pointer(root).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let (x, y) = (i32::from(ptr.root_x), i32::from(ptr.root_y));
        let cancelled = escape.is_some_and(|k| conn.query_keymap().ok().and_then(|c| c.reply().ok()).is_some_and(|m| m.keys[usize::from(k) / 8] & (1 << (k % 8)) != 0));
        let button_down = ptr.mask.contains(KeyButMask::BUTTON1);
        // Replies from the target (Status) and requests for the data.
        while let Some(event) = conn.poll_for_event().map_err(|e| e.to_string())? {
            match event {
                Event::ClientMessage(m) if m.type_ == at.status => accepted = status_accepts(m.data.as_data32()[1]),
                Event::SelectionRequest(r) if r.selection == at.selection => serve(&conn, &at, &r, &data),
                _ => {}
            }
        }
        if cancelled {
            if let Some((t, _)) = current {
                send(&conn, t, message(t, at.leave, [source, 0, 0, 0, 0]));
            }
            break false;
        }
        let found = target_under(&conn, root, &at, x, y);
        if found.map(|f| f.0) != current.map(|c| c.0) {
            if let Some((t, _)) = current {
                send(&conn, t, message(t, at.leave, [source, 0, 0, 0, 0]));
            }
            accepted = false;
            last_sent = (-1, -1);
            current = found;
            if let Some((t, version)) = current {
                send(&conn, t, message(t, at.enter, [source, version << 24, at.uri_list, NONE, NONE]));
            }
        }
        if !button_down {
            match current {
                Some((t, _)) if accepted => {
                    send(&conn, t, message(t, at.drop, [source, 0, CURRENT_TIME, 0, 0]));
                    dropped = true;
                    let version = current.map_or(0, |c| c.1);
                    // The target asks for the data and, from version 2 on, says it is done.
                    let waited = Instant::now();
                    while waited.elapsed() < DROP_WAIT {
                        match conn.poll_for_event().map_err(|e| e.to_string())? {
                            Some(Event::SelectionRequest(r)) if r.selection == at.selection => serve(&conn, &at, &r, &data),
                            Some(Event::ClientMessage(m)) if m.type_ == at.finished => break,
                            Some(_) => {}
                            None if version < 2 && waited.elapsed() > Duration::from_millis(500) => break,
                            None => std::thread::sleep(Duration::from_millis(5)),
                        }
                    }
                }
                Some((t, _)) => send(&conn, t, message(t, at.leave, [source, 0, 0, 0, 0])),
                None => {}
            }
            break dropped;
        }
        if let Some((t, _)) = current {
            if (x, y) != last_sent && last_position.elapsed() >= TICK {
                send(&conn, t, message(t, at.position, [source, 0, pack_position(x, y), CURRENT_TIME, at.copy]));
                last_sent = (x, y);
                last_position = Instant::now();
            }
        }
        std::thread::sleep(TICK);
    };
    let _ = conn.destroy_window(source);
    let _ = conn.flush();
    Ok(outcome)
}

/// Answers a request for the dragged data: the link as `text/uri-list`, or the list of what is
/// offered; anything else is refused (property `None`).
fn serve(conn: &RustConnection, at: &Atoms, r: &x11rb::protocol::xproto::SelectionRequestEvent, data: &[u8]) {
    let property = if r.property == NONE { r.target } else { r.property };
    let answered = if r.target == at.uri_list {
        conn.change_property8(PropMode::REPLACE, r.requestor, property, at.uri_list, data).is_ok()
    } else if r.target == at.targets {
        conn.change_property32(PropMode::REPLACE, r.requestor, property, AtomEnum::ATOM, &[at.targets, at.uri_list]).is_ok()
    } else {
        false
    };
    let event = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: r.time,
        requestor: r.requestor,
        selection: r.selection,
        target: r.target,
        property: if answered { property } else { NONE },
    };
    let _ = conn.send_event(false, r.requestor, EventMask::NO_EVENT, event);
    let _ = conn.flush();
}

/// The drag for the editor's drag handle: the environment's display; the file goes when the drag ends.
pub fn drag(path: &Path) -> Result<(), String> {
    let dropped = drag_on(None, path)?;
    crate::dragout::finish(path, dropped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use x11rb::protocol::xtest::ConnectionExt as _;

    /// A private X server for the test (never the user's display: the test fakes mouse input).
    struct Xvfb {
        child: std::process::Child,
        display: String,
    }

    impl Xvfb {
        fn start() -> Option<Xvfb> {
            use std::io::BufRead;
            let mut child = std::process::Command::new("Xvfb")
                .args(["-displayfd", "1", "-screen", "0", "800x600x24", "-nolisten", "tcp"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
            let n: u32 = line.trim().parse().ok()?;
            Some(Xvfb { child, display: format!(":{n}") })
        }
    }

    impl Drop for Xvfb {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// A window that takes drops the XDND way: accepts positions, asks for `text/uri-list` on the
    /// drop, and says it is finished. Returns what it was given.
    fn target(display: String, got: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>, ready: std::sync::mpsc::Sender<()>) {
        let (conn, n) = x11rb::connect(Some(&display)).expect("target connects");
        let root = conn.setup().roots[n].root;
        let at = atoms(&conn).expect("atoms");
        let win = conn.generate_id().unwrap();
        conn.create_window(COPY_DEPTH_FROM_PARENT, win, root, 100, 100, 200, 200, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new()).unwrap();
        conn.change_property32(PropMode::REPLACE, win, at.aware, AtomEnum::ATOM, &[5]).unwrap();
        conn.map_window(win).unwrap();
        conn.flush().unwrap();
        let _ = ready.send(());
        let property = at.uri_list;
        let mut source = 0;
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            match conn.poll_for_event().unwrap() {
                Some(Event::ClientMessage(m)) if m.type_ == at.enter => source = m.data.as_data32()[0],
                Some(Event::ClientMessage(m)) if m.type_ == at.position => send(&conn, source, message(source, at.status, [win, 1 | 2, 0, 0, at.copy])),
                Some(Event::ClientMessage(m)) if m.type_ == at.drop => {
                    conn.convert_selection(win, at.selection, at.uri_list, property, CURRENT_TIME).unwrap();
                    conn.flush().unwrap();
                }
                Some(Event::SelectionNotify(n)) if n.property != NONE => {
                    let reply = conn.get_property(true, win, property, AtomEnum::ANY, 0, 1024).unwrap().reply().unwrap();
                    *got.lock().unwrap() = Some(reply.value);
                    send(&conn, source, message(source, at.finished, [win, 1, at.copy, 0, 0]));
                    return;
                }
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    #[test]
    fn a_drag_ends_with_the_file_link_arriving_at_the_window_under_the_pointer() {
        let Some(server) = Xvfb::start() else {
            eprintln!("no Xvfb: skipped");
            return;
        };
        let path = std::env::temp_dir().join(format!("shotcraft xdnd test {}.png", std::process::id()));
        std::fs::write(&path, b"x").unwrap();
        let got: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>> = Default::default();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let target_thread = {
            let (display, got) = (server.display.clone(), got.clone());
            std::thread::spawn(move || target(display, got, ready_tx))
        };
        ready_rx.recv_timeout(Duration::from_secs(10)).expect("the target window is up");

        // The fake mouse: press outside the target, then drag over it and let go.
        let (conn, n) = x11rb::connect(Some(&server.display)).expect("driver connects");
        let root = conn.setup().roots[n].root;
        let fake = |kind: u8, detail: u8, x: i16, y: i16| {
            conn.xtest_fake_input(kind, detail, 0, root, x, y, 0).unwrap();
            conn.flush().unwrap();
        };
        fake(6, 0, 10, 10);
        fake(4, 1, 0, 0);
        let (display, drag_path) = (server.display.clone(), path.clone());
        let dragger = std::thread::spawn(move || drag_on(Some(&display), &drag_path));
        std::thread::sleep(Duration::from_millis(300));
        for step in 1..=10i16 {
            fake(6, 0, 10 + step * 14, 10 + step * 14);
            std::thread::sleep(Duration::from_millis(40));
        }
        std::thread::sleep(Duration::from_millis(300));
        fake(5, 1, 0, 0);

        let dropped = dragger.join().expect("the drag ends").expect("no error");
        target_thread.join().expect("the target ends");
        assert!(dropped, "the target accepted the drop");
        let list = got.lock().unwrap().clone().expect("the target was given the data");
        assert_eq!(String::from_utf8_lossy(&list), format!("{}\r\n", crate::dragout::file_uri(&path)));
        assert!(list.starts_with(b"file:///") && list.windows(3).any(|w| w == b"%20"), "a space in the name is escaped");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn escape_cancels_a_drag_over_a_target_and_nothing_is_dropped() {
        let Some(server) = Xvfb::start() else {
            eprintln!("no Xvfb: skipped");
            return;
        };
        let path = std::env::temp_dir().join(format!("shotcraft-xdnd-cancel-{}.png", std::process::id()));
        std::fs::write(&path, b"x").unwrap();
        let got: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>> = Default::default();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        {
            let (display, got) = (server.display.clone(), got.clone());
            std::thread::spawn(move || target(display, got, ready_tx));
        }
        ready_rx.recv_timeout(Duration::from_secs(10)).expect("the target window is up");
        let (conn, n) = x11rb::connect(Some(&server.display)).expect("driver connects");
        let root = conn.setup().roots[n].root;
        let esc = escape_keycode(&conn).expect("the keyboard has Escape");
        let fake = |kind: u8, detail: u8, x: i16, y: i16| {
            conn.xtest_fake_input(kind, detail, 0, root, x, y, 0).unwrap();
            conn.flush().unwrap();
        };
        fake(6, 0, 10, 10);
        fake(4, 1, 0, 0);
        let (display, drag_path) = (server.display.clone(), path.clone());
        let dragger = std::thread::spawn(move || drag_on(Some(&display), &drag_path));
        std::thread::sleep(Duration::from_millis(200));
        for step in 1..=6i16 {
            fake(6, 0, 10 + step * 24, 10 + step * 24);
            std::thread::sleep(Duration::from_millis(40));
        }
        std::thread::sleep(Duration::from_millis(200));
        fake(2, esc, 0, 0);
        let dropped = dragger.join().expect("the drag ends").expect("no error");
        fake(5, 1, 0, 0);
        fake(3, esc, 0, 0);
        assert!(!dropped, "Esc cancelled it");
        std::thread::sleep(Duration::from_millis(200));
        assert!(got.lock().unwrap().is_none(), "the target was given nothing");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_status_accepts_with_bit_zero_and_versions_are_capped() {
        assert!(status_accepts(1) && status_accepts(3) && !status_accepts(0) && !status_accepts(2));
        assert_eq!((negotiated(3), negotiated(5), negotiated(9)), (3, 5, 5));
    }

    #[test]
    fn a_position_packs_x_over_y_and_never_goes_negative() {
        assert_eq!(pack_position(0x12, 0x34), 0x0012_0034);
        assert_eq!(pack_position(-5, 7), 7);
        assert_eq!(pack_position(70_000, 1), 0xFFFF_0001);
    }
}
