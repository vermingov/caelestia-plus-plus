//! What the shell is told, and what it may ask for.
//!
//! One part of the desktop cannot be drawn from here: the lock screen. It is
//! an `ext-session-lock` surface, and while one is up the compositor shows
//! nothing else — no layer surface of the bar's can appear above it. So the
//! notifications on the lock screen are still the shell's to draw, and the
//! shell's keybinds (`sidebar`, `clearNotifs`) are still the shell's to hear.
//!
//! This is the line between the two: a Unix socket speaking the same contract
//! as the guards' — newline-delimited JSON one way, one command per line the
//! other. A client is sent the whole feed when it connects and again whenever
//! it changes.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{reason, Notifs};

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    runtime.join("caelestia-notifs.sock")
}

/// Everyone currently listening. A client that has gone away is found out the
/// next time something is written to it, and dropped then.
type Clients = Arc<Mutex<Vec<UnixStream>>>;

pub(super) fn listen(notifs: Notifs) {
    let path = socket_path();
    // Left over from a bar that was killed rather than stopped. Nothing can
    // be listening on it: there is only ever one bar, and this is it.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("caelestia-bar: the shell cannot be told about notifications: {e}");
            return;
        }
    };

    let clients: Clients = Arc::default();

    let listening = clients.clone();
    notifs.on_change(move |feed| {
        let Ok(line) = serde_json::to_string(feed) else { return };
        let Ok(mut clients) = listening.lock() else { return };
        clients.retain_mut(|client| writeln!(client, "{line}").is_ok());
    });

    std::thread::spawn(move || {
        for stream in listener.incoming().filter_map(Result::ok) {
            greet(&notifs, &clients, stream);
        }
    });
}

/// Sends a new client the feed as it stands and starts listening to it.
fn greet(notifs: &Notifs, clients: &Clients, mut stream: UnixStream) {
    let Ok(line) = serde_json::to_string(&notifs.feed()) else { return };
    if writeln!(stream, "{line}").is_err() {
        return;
    }
    let Ok(reader) = stream.try_clone() else { return };
    if let Ok(mut clients) = clients.lock() {
        clients.push(stream);
    }

    let notifs = notifs.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            obey(&notifs, line.trim());
        }
    });
}

/// One command from the shell.
///
/// `centre` opens on whichever output has the focus, because a keybind does
/// not say where it was pressed and that is where the person is looking.
fn obey(notifs: &Notifs, line: &str) {
    let (verb, argument) = line.split_once(' ').unwrap_or((line, ""));
    match (verb, argument.trim()) {
        ("clear", _) => notifs.clear(),
        ("close", id) => {
            if let Ok(id) = id.parse() {
                notifs.close(id, reason::DISMISSED);
            }
        }
        ("dnd", "on") => notifs.set_dnd(true),
        ("dnd", "off") => notifs.set_dnd(false),
        ("dnd", _) => notifs.toggle_dnd(),
        ("centre", "open") => notifs.set_centre(&focused_output()),
        ("centre", "close") => notifs.set_centre(""),
        ("centre", _) => notifs.toggle_centre(&focused_output()),
        _ => {}
    }
}

fn focused_output() -> String {
    crate::hypr::focused_monitor().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifs::{store::Store, Notification};

    fn server_with_two() -> (Notifs, u32) {
        let notifs = Notifs::with_store(Store::nowhere());
        notifs.accept(Notification { summary: "one".into(), ..Notification::default() }, 0);
        let second = notifs.accept(Notification { summary: "two".into(), ..Notification::default() }, 0);
        (notifs, second)
    }

    #[test]
    fn the_shell_can_close_one_and_clear_the_rest() {
        let (notifs, second) = server_with_two();

        obey(&notifs, &format!("close {second}"));
        let list = notifs.feed().list;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].summary, "one");

        obey(&notifs, "clear");
        assert!(notifs.feed().list.is_empty());
    }

    #[test]
    fn do_not_disturb_is_set_or_toggled() {
        let (notifs, _) = server_with_two();
        obey(&notifs, "dnd on");
        assert!(notifs.feed().dnd);
        obey(&notifs, "dnd on");
        assert!(notifs.feed().dnd, "asking for it twice turned it off");
        obey(&notifs, "dnd toggle");
        assert!(!notifs.feed().dnd);
    }

    #[test]
    fn nonsense_is_ignored() {
        let (notifs, _) = server_with_two();
        obey(&notifs, "close not-a-number");
        obey(&notifs, "");
        obey(&notifs, "launch the missiles");
        assert_eq!(notifs.feed().list.len(), 2);
    }

    /// A client is told what there is the moment it connects, and again when
    /// that changes — over a real socket, in a directory of the test's own.
    #[test]
    fn a_client_is_sent_the_feed_and_then_every_change() {
        let dir = std::env::temp_dir().join(format!("caelestia-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notifs.sock");

        let (notifs, _) = server_with_two();
        let listener = UnixListener::bind(&path).unwrap();
        let clients: Clients = Arc::default();

        let listening = clients.clone();
        notifs.on_change(move |feed| {
            let line = serde_json::to_string(feed).unwrap();
            listening.lock().unwrap().retain_mut(|client| writeln!(client, "{line}").is_ok());
        });

        let serving = notifs.clone();
        let accepting = clients.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().filter_map(Result::ok) {
                greet(&serving, &accepting, stream);
            }
        });

        let client = UnixStream::connect(&path).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let mut lines = BufReader::new(client.try_clone().unwrap()).lines();

        let first: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(first["list"].as_array().unwrap().len(), 2, "the greeting was not the whole feed");

        // A command sent down the same socket changes the state, and the
        // change comes straight back.
        writeln!(&client, "clear").unwrap();
        let second: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert!(second["list"].as_array().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
