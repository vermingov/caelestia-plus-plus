//! The UI socket: newline-delimited JSON over a Unix socket at 0660
//! root:ui_gid, spoken by the Quickshell widget in the bar.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::firewall::Firewall;
use crate::json::{self, Json};
use crate::rules::Action;

struct Client {
    id: u64,
    stream: UnixStream,
}

/// The connected UIs. Broadcasting to a dead socket is normal (the shell
/// reloads), so a failed write just drops that client.
pub struct Clients {
    inner: Mutex<Vec<Client>>,
    next_id: AtomicU64,
    /// Only set by unit tests, which have no real socket to connect.
    test_connected: AtomicBool,
}

impl Clients {
    pub fn new() -> Clients {
        Clients {
            inner: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            test_connected: AtomicBool::new(false),
        }
    }

    pub fn any_connected(&self) -> bool {
        if self.test_connected.load(Ordering::Relaxed) {
            return true;
        }
        !self.inner.lock().unwrap().is_empty()
    }

    #[cfg(test)]
    pub fn set_test_connected(&self, v: bool) {
        self.test_connected.store(v, Ordering::Relaxed);
    }

    pub fn broadcast(&self, msg: &Json) {
        let line = format!("{}\n", msg.dump());
        let mut clients = self.inner.lock().unwrap();
        clients.retain_mut(|c| c.stream.write_all(line.as_bytes()).is_ok());
    }

    fn add(&self, stream: UnixStream) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.lock().unwrap().push(Client { id, stream });
        id
    }

    fn remove(&self, id: u64) -> bool {
        let mut clients = self.inner.lock().unwrap();
        clients.retain(|c| c.id != id);
        clients.is_empty()
    }

    fn send_to(&self, id: u64, msg: &Json) {
        let line = format!("{}\n", msg.dump());
        let mut clients = self.inner.lock().unwrap();
        if let Some(c) = clients.iter_mut().find(|c| c.id == id) {
            let _ = c.stream.write_all(line.as_bytes());
        }
    }
}

pub fn serve(fw: Arc<Firewall>, sock_path: &str, ui_gid: u32) -> std::io::Result<()> {
    if let Some(dir) = std::path::Path::new(sock_path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(sock_path);

    let listener = UnixListener::bind(sock_path)?;
    chown_and_mode(sock_path, ui_gid);

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let fw = Arc::clone(&fw);
        thread::spawn(move || handle_client(fw, stream, ui_gid));
    }
    Ok(())
}

fn handle_client(fw: Arc<Firewall>, stream: UnixStream, ui_gid: u32) {
    if !authorized(&stream, ui_gid) {
        eprintln!("[redwall] refused a UI connection from an unauthorised peer");
        return;
    }
    let Ok(write_half) = stream.try_clone() else { return };
    let id = fw.clients.add(write_half);

    // Catch a freshly-launched bar up: current rules, current state, and any
    // prompt already waiting for an answer.
    fw.clients.send_to(
        id,
        &json::obj([("t", json::s("rules")), ("rules", fw.rules_snapshot())]),
    );
    fw.clients.send_to(
        id,
        &json::obj([("t", json::s("state")), ("enabled", Json::Bool(fw.enabled()))]),
    );
    for ask in fw.waiting_asks() {
        fw.clients.send_to(id, &ask);
    }

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Some(msg) = json::parse(&line) else {
            continue; // malformed input from a trusted peer is ignored, not fatal
        };
        dispatch(&fw, &msg);
    }

    // No UI left means nobody can answer, and handle() already passes new
    // packets straight through in that state — so anything still held has to
    // go through too, or a shell reload would strand it.
    if fw.clients.remove(id) {
        fw.release_all("no UI connected");
    }
}

fn dispatch(fw: &Firewall, msg: &Json) {
    match msg.str_field("t") {
        Some("verdict") => {
            let Some(id) = msg.get("id").and_then(Json::as_u64) else { return };
            let action = Action::parse(msg.str_field("action").unwrap_or("deny"));
            fw.apply_verdict(id, action, msg.bool_field("remember", true));
        }
        Some("setrule") => {
            let Some(exe) = msg.str_field("exe") else { return };
            let action = Action::parse(msg.str_field("action").unwrap_or("deny"));
            fw.set_rule(exe, action, msg.str_field("name"));
        }
        Some("delrule") => {
            if let Some(exe) = msg.str_field("exe") {
                fw.delete_rule(exe);
            }
        }
        Some("getrules") => fw.push_rules(),
        Some("setenabled") => fw.set_enabled(msg.bool_field("enabled", true)),
        _ => {}
    }
}

/// Defence in depth over the socket's 0660 root:ui_gid mode: confirm the peer
/// really is root or in ui_gid. This mirrors the grant the kernel already made
/// when it allowed the connect, so it never rejects a legitimate UI — but it
/// refuses anyone else should the socket ever be left more permissive.
fn authorized(stream: &UnixStream, ui_gid: u32) -> bool {
    #[repr(C)]
    struct Ucred {
        pid: i32,
        uid: u32,
        gid: u32,
    }
    const SOL_SOCKET: i32 = 1;
    const SO_PEERCRED: i32 = 17;

    let mut cred = Ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<Ucred>() as u32;
    let rc = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            SOL_SOCKET,
            SO_PEERCRED,
            &mut cred as *mut _ as *mut u8,
            &mut len,
        )
    };
    if rc != 0 {
        return false;
    }
    cred.uid == 0 || cred.gid == ui_gid
}

fn chown_and_mode(path: &str, ui_gid: u32) {
    let c_path = std::ffi::CString::new(path).unwrap();
    unsafe {
        // Best effort: if the chown fails the mode below still keeps it off
        // the world, and the peercred check above is the real gate.
        chown(c_path.as_ptr(), 0, ui_gid);
        chmod(c_path.as_ptr(), 0o660);
    }
}

extern "C" {
    fn getsockopt(fd: i32, level: i32, name: i32, val: *mut u8, len: *mut u32) -> i32;
    fn chown(path: *const i8, uid: u32, gid: u32) -> i32;
    fn chmod(path: *const i8, mode: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dead_client_is_dropped_rather_than_retried() {
        let clients = Clients::new();
        let (a, b) = UnixStream::pair().unwrap();
        clients.add(a);
        assert!(clients.any_connected());

        drop(b); // the bar went away
        // The first write may land in the socket buffer; the second cannot.
        for _ in 0..2 {
            clients.broadcast(&json::obj([("t", json::s("ping"))]));
        }
        assert!(!clients.any_connected(), "dead client removed");
    }

    #[test]
    fn broadcast_reaches_a_live_client_as_one_json_line() {
        let clients = Clients::new();
        let (a, b) = UnixStream::pair().unwrap();
        clients.add(a);
        clients.broadcast(&json::obj([("t", json::s("state")), ("enabled", Json::Bool(true))]));

        let mut line = String::new();
        BufReader::new(b).read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'));
        let parsed = json::parse(line.trim()).unwrap();
        assert_eq!(parsed.str_field("t"), Some("state"));
        assert_eq!(parsed.get("enabled").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn malformed_messages_do_not_take_down_the_handler() {
        let dir = std::env::temp_dir().join(format!("redwall-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fw = Firewall::new(
            None,
            crate::rules::Rules::load(dir.join("r.json")),
            crate::rules::State::load(dir.join("s.json")),
        );
        // None of these should panic
        dispatch(&fw, &json::obj([("t", json::s("verdict"))]));
        dispatch(&fw, &json::obj([("t", json::s("setrule"))]));
        dispatch(&fw, &json::obj([("t", json::s("nonsense"))]));
        dispatch(&fw, &Json::Null);
    }
}
