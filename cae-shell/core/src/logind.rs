//! What the login manager says about this session.
//!
//! Three things matter to a shell: somebody has asked for the screen to be
//! locked (`loginctl lock-session`, which is also what a lid or a key can be
//! wired to), somebody has asked for it to be unlocked, and the machine is
//! about to sleep. The third is why a locked machine does not wake up
//! showing what was on the screen before it slept.
//!
//! The unlock signal is also the way back in when something has gone wrong
//! with the lock screen itself: from a terminal, on another seat or over
//! ssh, `loginctl unlock-session` reaches here.

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

const MANAGER: (&str, &str, &str) =
    ("org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager");
const SESSION: &str = "org.freedesktop.login1.Session";

/// What the manager said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    Lock,
    Unlock,
    /// The machine is going to sleep, and has not yet.
    Sleeping,
}

/// Calls `heard` for each of them, for as long as the session lasts.
/// Blocking; meant for a thread of its own.
///
/// One thread per signal, because a blocking stream blocks: waiting on one
/// of them would mean hearing nothing from the other two.
pub fn watch(mut heard: impl FnMut(Said)) {
    let Ok(bus) = Connection::system() else { return eprintln!("cae: no system bus, so nothing is heard from logind") };

    // Sleep is the machine's, not the session's: it is heard whether or not
    // a session is found, so a lid that closes still locks the screen first.
    let mut listeners = vec![(MANAGER.1.to_string(), MANAGER.2, "PrepareForSleep")];
    match this_session(&bus) {
        Some((_, session)) => {
            listeners.push((session.clone(), SESSION, "Lock"));
            listeners.push((session, SESSION, "Unlock"));
        }
        None => eprintln!("cae: logind knows no graphical session of this user's, so only sleep is heard from it"),
    }

    let (said, from_logind) = std::sync::mpsc::channel();
    for (path, interface, signal) in listeners {
        let (bus, said) = (bus.clone(), said.clone());
        std::thread::Builder::new()
            .name(format!("logind-{signal}"))
            .spawn(move || {
                let Ok(proxy) = Proxy::new(&bus, MANAGER.0, path, interface) else { return };
                let Ok(messages) = proxy.receive_signal(signal) else { return };
                for message in messages {
                    let what = match signal {
                        "Lock" => Said::Lock,
                        "Unlock" => Said::Unlock,
                        // True on the way down and false on the way back up;
                        // only the first is a moment to do anything about.
                        _ if message.body().deserialize::<bool>().unwrap_or(false) => Said::Sleeping,
                        _ => continue,
                    };
                    if said.send(what).is_err() {
                        return;
                    }
                }
            })
            .ok();
    }
    drop(said);

    for what in from_logind {
        heard(what);
    }
}

/// The id of the graphical session this shell draws for, which is what
/// `loginctl terminate-session` is to be told when logging out.
pub fn session_id() -> Option<String> {
    this_session(&Connection::system().ok()?).map(|(id, _)| id)
}

/// The graphical session this shell draws for, by id and by path.
///
/// Started inside the session, the shell is handed its id, and logind knows
/// the session by its pid besides. Started by the user's service manager,
/// which is how `cae-shell.service` runs it, it is in no session at all: its
/// environment has no id, logind answers "does not belong to any known
/// session" for its pid, and the session it draws for is the one logind
/// keeps as the user's display.
fn this_session(bus: &Connection) -> Option<(String, String)> {
    let manager = Proxy::new(bus, MANAGER.0, MANAGER.1, MANAGER.2).ok()?;
    let path_of = |id: &str| manager.call::<_, _, OwnedObjectPath>("GetSession", &(id)).ok();

    // By the id the session hands every process in it, so that a machine
    // with two sessions open is not a machine where the wrong one is locked.
    if let Ok(id) = std::env::var("XDG_SESSION_ID")
        && let Some(path) = path_of(&id)
    {
        return Some((id, path.to_string()));
    }
    if let Ok(path) = manager.call::<_, _, OwnedObjectPath>("GetSessionByPID", &(std::process::id()))
        && let Ok(session) = Proxy::new(bus, MANAGER.0, path.as_str(), SESSION)
        && let Ok(id) = session.get_property::<String>("Id")
    {
        return Some((id, path.to_string()));
    }
    // SAFETY: `getuid` cannot fail and touches nothing.
    let user = manager.call::<_, _, OwnedObjectPath>("GetUser", &(unsafe { libc::getuid() })).ok()?;
    let user = Proxy::new(bus, MANAGER.0, user.as_str(), "org.freedesktop.login1.User").ok()?;
    let (id, path) = user.get_property::<(String, OwnedObjectPath)>("Display").ok()?;
    (!id.is_empty()).then(|| (id, path.to_string()))
}
