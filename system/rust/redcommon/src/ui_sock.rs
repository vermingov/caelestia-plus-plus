//! The UI socket both daemons expose: newline-delimited JSON over a Unix
//! socket at 0660 root:ui_gid, spoken by the Quickshell widget in the bar.
//!
//! The daemon supplies a [`Handler`]; everything about connections, peer
//! authorisation, framing and broadcast lives here.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::json::Json;
use crate::warn;

/// What a daemon has to provide to be driven by this server.
pub trait Handler: Send + Sync + 'static {
    /// Catch a freshly-connected UI up on whatever state it has missed.
    fn on_connect(&self, clients: &Clients, client: u64);
    /// One decoded message from a UI. Malformed input never reaches here.
    fn on_message(&self, msg: &Json);
    /// The last UI went away, so nothing can be answered any more.
    fn on_idle(&self);
}

struct Client {
    id: u64,
    stream: UnixStream,
}

/// The connected UIs. Broadcasting to a dead socket is routine — the shell
/// reloads — so a failed write just drops that client.
pub struct Clients {
    inner: Mutex<Vec<Client>>,
    next_id: AtomicU64,
    /// Only set by unit tests, which have no real socket to connect.
    test_connected: AtomicBool,
}

impl Default for Clients {
    fn default() -> Self {
        Self::new()
    }
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

    /// Lets tests exercise the "a UI is watching" branches without a socket.
    pub fn set_test_connected(&self, v: bool) {
        self.test_connected.store(v, Ordering::Relaxed);
    }

    pub fn broadcast(&self, msg: &Json) {
        let line = format!("{}\n", msg.dump());
        let mut clients = self.inner.lock().unwrap();
        clients.retain_mut(|c| c.stream.write_all(line.as_bytes()).is_ok());
    }

    pub fn send_to(&self, id: u64, msg: &Json) {
        let line = format!("{}\n", msg.dump());
        let mut clients = self.inner.lock().unwrap();
        if let Some(c) = clients.iter_mut().find(|c| c.id == id) {
            let _ = c.stream.write_all(line.as_bytes());
        }
    }

    pub fn add(&self, stream: UnixStream) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.lock().unwrap().push(Client { id, stream });
        id
    }

    /// Returns true when that was the last one.
    fn remove(&self, id: u64) -> bool {
        let mut clients = self.inner.lock().unwrap();
        clients.retain(|c| c.id != id);
        clients.is_empty()
    }
}

pub fn serve<H: Handler>(
    handler: Arc<H>,
    clients: Arc<Clients>,
    sock_path: &str,
    ui_gid: u32,
) -> std::io::Result<()> {
    if let Some(dir) = std::path::Path::new(sock_path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(sock_path);

    let listener = std::os::unix::net::UnixListener::bind(sock_path)?;
    chown_and_mode(sock_path, ui_gid);

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let handler = Arc::clone(&handler);
        let clients = Arc::clone(&clients);
        thread::spawn(move || handle_client(handler, clients, stream, ui_gid));
    }
    Ok(())
}

fn handle_client<H: Handler>(
    handler: Arc<H>,
    clients: Arc<Clients>,
    stream: UnixStream,
    ui_gid: u32,
) {
    if !authorized(&stream, ui_gid) {
        return; // authorized() has already said who was turned away
    }
    let Ok(write_half) = stream.try_clone() else { return };
    let id = clients.add(write_half);
    handler.on_connect(&clients, id);

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        // Malformed input from a trusted peer is ignored, never fatal: a bad
        // message must not take the daemon's UI down with it.
        if let Some(msg) = crate::json::parse(&line) {
            handler.on_message(&msg);
        }
    }

    if clients.remove(id) {
        handler.on_idle();
    }
}

/// Defence in depth over the socket's 0660 root:ui_gid mode: confirm the peer
/// really is root or in ui_gid. This mirrors the grant the kernel already made
/// when it allowed the connect, so it never rejects a legitimate UI — but it
/// refuses, and says so, should the socket ever be left more permissive.
fn authorized(stream: &UnixStream, ui_gid: u32) -> bool {
    let Some(peer) = peer_cred(stream) else {
        warn!("refused a UI connection with no readable credentials");
        return false;
    };
    if peer_allowed(peer.uid, peer.gid, peer.pid as u32, ui_gid) {
        return true;
    }
    warn!(
        "rejected UI connection uid={} gid={} pid={}",
        peer.uid, peer.gid, peer.pid
    );
    false
}

/// The socket's group is only the peer's *primary* group. A desktop session
/// commonly has the granted group as a supplementary one instead, so a check
/// that stopped at the primary gid would turn away the very UI it exists for.
fn peer_allowed(uid: u32, gid: u32, pid: u32, ui_gid: u32) -> bool {
    uid == 0
        || gid == ui_gid
        || crate::procfs::supplementary_groups(pid).contains(&ui_gid)
}

#[repr(C)]
struct Ucred {
    pid: i32,
    uid: u32,
    gid: u32,
}

fn peer_cred(stream: &UnixStream) -> Option<Ucred> {
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
    (rc == 0).then_some(cred)
}

fn chown_and_mode(path: &str, ui_gid: u32) {
    let Ok(c_path) = std::ffi::CString::new(path) else { return };
    unsafe {
        // Best effort: if the chown fails the mode still keeps it off the
        // world, and the peercred check above is the real gate.
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
    use crate::json;

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
        clients.broadcast(&json::obj([
            ("t", json::s("state")),
            ("enabled", Json::Bool(true)),
        ]));

        let mut line = String::new();
        BufReader::new(b).read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'));
        let parsed = json::parse(line.trim()).unwrap();
        assert_eq!(parsed.str_field("t"), Some("state"));
        assert_eq!(parsed.get("enabled").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn send_to_addresses_one_client_only() {
        let clients = Clients::new();
        let (a, mut a_peer) = UnixStream::pair().unwrap();
        let (b, b_peer) = UnixStream::pair().unwrap();
        let id_a = clients.add(a);
        clients.add(b);

        clients.send_to(id_a, &json::obj([("t", json::s("only-a"))]));

        let mut line = String::new();
        BufReader::new(&mut a_peer).read_line(&mut line).unwrap();
        assert!(line.contains("only-a"));

        b_peer
            .set_read_timeout(Some(std::time::Duration::from_millis(50)))
            .unwrap();
        let mut other = String::new();
        assert!(
            BufReader::new(b_peer).read_line(&mut other).is_err() || other.is_empty(),
            "the other client heard nothing"
        );
    }

    #[test]
    fn root_and_the_granted_group_get_in_and_nobody_else() {
        assert!(peer_allowed(0, 999, 1, 1000), "root always");
        assert!(peer_allowed(1000, 1000, 1, 1000), "primary group matches");
        // pid 0 can never exist, so there are no supplementary groups to save it
        assert!(!peer_allowed(1000, 999, 0, 1000), "wrong group, no membership");
    }

    #[test]
    fn a_supplementary_group_is_enough() {
        // Our own process is the only peer whose groups this test can know.
        let me = std::process::id();
        let Some(&group) = crate::procfs::supplementary_groups(me).first() else {
            return; // a session with no supplementary groups proves nothing
        };
        assert!(
            peer_allowed(1000, group + 1, me, group),
            "membership counts even when it is not the primary group"
        );
    }

    #[test]
    fn peercred_reads_our_own_side_of_a_socket_pair() {
        let (a, _b) = UnixStream::pair().unwrap();
        let cred = peer_cred(&a).expect("SO_PEERCRED on a socketpair");
        assert_eq!(cred.pid as u32, std::process::id());
        assert!(authorized(&a, cred.gid), "we would admit ourselves");
    }
}
