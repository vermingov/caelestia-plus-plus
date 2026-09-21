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

const MANAGER: (&str, &str, &str) =
    ("org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager");

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
    let Some(session) = this_session(&bus) else { return eprintln!("cae: logind does not know this session") };

    let (said, from_logind) = std::sync::mpsc::channel();
    let listeners = [
        (session.clone(), "org.freedesktop.login1.Session", "Lock"),
        (session, "org.freedesktop.login1.Session", "Unlock"),
        (MANAGER.1.to_string(), MANAGER.2, "PrepareForSleep"),
    ];
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

/// The path of the session this process is in.
fn this_session(bus: &Connection) -> Option<String> {
    let manager = Proxy::new(bus, MANAGER.0, MANAGER.1, MANAGER.2).ok()?;
    // By the id the session hands every process in it, so that a machine
    // with two sessions open is not a machine where the wrong one is locked.
    if let Ok(id) = std::env::var("XDG_SESSION_ID")
        && let Ok(path) = manager.call_method("GetSession", &(id.as_str()))
        && let Ok(path) = path.body().deserialize::<zbus::zvariant::OwnedObjectPath>()
    {
        return Some(path.as_str().to_string());
    }
    let path = manager.call_method("GetSessionByPID", &(std::process::id())).ok()?;
    let path = path.body().deserialize::<zbus::zvariant::OwnedObjectPath>().ok()?;
    Some(path.as_str().to_string())
}
