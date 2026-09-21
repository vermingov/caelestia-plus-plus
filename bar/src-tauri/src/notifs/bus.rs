//! `org.freedesktop.Notifications`, as the rest of the desktop sees it.
//!
//! A thin translation: the methods turn what arrives on the wire into a
//! `Notification` and hand it to the state in `mod.rs`, which is where every
//! decision is made. What is here besides that is getting hold of the bus
//! name, which on a real desktop is the hard part.

use std::collections::HashMap;
use std::time::Duration;

use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::OwnedValue;

use super::{images, now_ms, reason, Action, Notification, Notifs};

pub const NAME: &str = "org.freedesktop.Notifications";
pub const PATH: &str = "/org/freedesktop/Notifications";

/// How long whoever holds the name is given to let go of it on their own
/// before they are looked at. The shell's old server does, the moment it sees
/// the bar is installed.
const GRACE: Duration = Duration::from_secs(3);

/// How often the name is checked on after that. Nothing can take it from the
/// owner, so this only matters while something else still holds it.
const RECHECK: Duration = Duration::from_secs(300);

pub(super) struct Server {
    pub notifs: Notifs,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    /// What this server can do, as the spec names them. A sender checks this
    /// before bothering to send markup or actions.
    fn get_capabilities(&self) -> Vec<&str> {
        vec![
            "actions",
            "action-icons",
            "body",
            "body-hyperlinks",
            "body-markup",
            "icon-static",
            "persistence",
        ]
    }

    fn get_server_information(&self) -> (&str, &str, &str, &str) {
        ("caelestia-bar", "caelestia", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let notif = Notification {
            id: 0,
            time: now_ms(),
            app_icon: images::app_icon(&app_icon, string_hint(&hints, "desktop-entry").as_deref()),
            app_name,
            summary,
            body,
            image: images::from_hints(&hints),
            urgency: byte_hint(&hints, "urgency").unwrap_or(1),
            resident: bool_hint(&hints, "resident"),
            transient: bool_hint(&hints, "transient"),
            has_action_icons: bool_hint(&hints, "action-icons"),
            expire_timeout,
            // The wire format is a flat list of alternating id and label.
            actions: actions
                .chunks_exact(2)
                .map(|pair| Action { identifier: pair[0].clone(), text: pair[1].clone() })
                .collect(),
            progress: number_hint(&hints, "value").map(|value| value.clamp(0, 100) as i32),
            popup: false,
        };
        self.notifs.accept(notif, replaces_id)
    }

    fn close_notification(&self, id: u32) {
        self.notifs.close(id, reason::CLOSED_BY_CALL);
    }

    /// Emitted when one goes away, whoever made that happen.
    #[zbus(signal)]
    async fn notification_closed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    /// Emitted when somebody presses one of the sender's buttons.
    #[zbus(signal)]
    async fn action_invoked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

fn bool_hint(hints: &HashMap<String, OwnedValue>, key: &str) -> bool {
    hints.get(key).and_then(|v| bool::try_from(v.clone()).ok()).unwrap_or(false)
}

fn byte_hint(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    hints.get(key).and_then(|v| u8::try_from(v.clone()).ok())
}

pub(super) fn string_hint(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    hints.get(key).and_then(|v| String::try_from(v.clone()).ok()).filter(|s| !s.is_empty())
}

/// A number, whichever integer type the sender happened to reach for. The
/// spec says `value` is an int32; what arrives is whatever the sender's
/// language made of "50".
fn number_hint(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let value = hints.get(key)?;
    i32::try_from(value.clone())
        .map(i64::from)
        .or_else(|_| u32::try_from(value.clone()).map(i64::from))
        .or_else(|_| i64::try_from(value.clone()))
        .or_else(|_| u8::try_from(value.clone()).map(i64::from))
        .ok()
}

/// Serves the interface and takes the name, on a thread of its own: taking
/// the name can mean waiting for somebody else to let go of it.
pub(super) fn serve(notifs: Notifs) {
    std::thread::spawn(move || {
        if let Err(e) = run(notifs) {
            eprintln!("caelestia-bar: the notification server could not start: {e}");
        }
    });
}

fn run(notifs: Notifs) -> zbus::Result<()> {
    let connection = zbus::blocking::connection::Builder::session()?
        .serve_at(PATH, Server { notifs: notifs.clone() })?
        .build()?;

    if let Ok(mut slot) = notifs.bus.lock() {
        *slot = Some(connection.clone());
    }

    // Asked for without `DoNotQueue`: if something else holds the name this
    // joins the queue for it, and is handed it the moment the holder lets go
    // or exits. Without `AllowReplacement`: once it is ours it stays ours,
    // so a daemon that respawns later cannot quietly take it back.
    let reply = connection.request_name_with_flags(NAME, RequestNameFlags::ReplaceExisting.into())?;
    if !matches!(reply, RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) {
        std::thread::sleep(GRACE);
        while !owns_name(&connection) {
            displace_current_owner(&connection);
            std::thread::sleep(RECHECK);
        }
    }

    // The connection has to outlive this function or the name goes with it.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn owns_name(connection: &zbus::blocking::Connection) -> bool {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(connection) else { return false };
    let Ok(name) = zbus::names::BusName::try_from(NAME) else { return false };
    let Ok(owner) = dbus.get_name_owner(name) else { return false };
    connection.unique_name().is_some_and(|ours| ours.as_str() == owner.as_str())
}

/// Processes that are never stopped to get the name, whatever else is true.
///
/// Displacing a notification daemon — dunst, mako, swaync — is the whole
/// point. But some desktops hand this name to the session shell itself, and
/// killing that takes the entire desktop down. The shell that runs this bar
/// is on the list too: its own server lets go by itself once it sees the bar
/// is installed, and stopping the shell would stop the bar with it.
const NEVER_STOPPED: [&str; 20] = [
    "Hyprland",
    "sway",
    "river",
    "niri",
    "labwc",
    "weston",
    "plasmashell",
    "kwin_wayland",
    "kwin_x11",
    "gnome-shell",
    "xfce4-session",
    "cinnamon-session",
    "mate-session",
    "lxqt-session",
    "systemd",
    "init",
    "qs",
    "quickshell",
    "caelestia-bar",
    // The shell that is taking the bar's place. While one hands over to the
    // other, each may find the other holding the name, and the answer is to
    // wait for it rather than to stop it.
    "cae-shell",
];

/// Cgroup units that belong to the session rather than to one daemon. A
/// notification daemon started from the compositor shares its unit, and
/// stopping that unit would stop the desktop.
const SESSION_UNITS: [&str; 7] =
    ["user@", "wayland-wm", "graphical-session", "hyprland", "plasma", "gnome-session", "init.scope"];

/// Stops whoever holds the name, if it is something that may be stopped.
///
/// This machine has been here before: a second daemon grabbed the name first
/// and every notification came out looking like 1998. The queue above means
/// nothing more is needed once the holder is gone.
fn displace_current_owner(connection: &zbus::blocking::Connection) {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(connection) else { return };
    let Ok(name) = zbus::names::BusName::try_from(NAME) else { return };
    let Ok(owner) = dbus.get_name_owner(name) else { return };
    let Ok(pid) = dbus.get_connection_unix_process_id(owner.into_inner().into()) else { return };
    if pid == std::process::id() {
        return;
    }

    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    let comm = comm.trim();
    if comm.is_empty() || NEVER_STOPPED.contains(&comm) {
        eprintln!("caelestia-bar: {comm} (pid {pid}) holds the notification service and is left alone");
        return;
    }

    if let Some(unit) = dedicated_unit(pid) {
        let _ = std::process::Command::new("systemctl").args(["--user", "stop", &unit]).status();
    }
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    eprintln!("caelestia-bar: stopped {comm} (pid {pid}), which held the notification service");
}

/// The systemd unit a process runs in, if it has one to itself.
fn dedicated_unit(pid: u32) -> Option<String> {
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    cgroup
        .split('/')
        .map(str::trim)
        .filter(|part| part.ends_with(".service"))
        .find(|unit| !SESSION_UNITS.iter().any(|session| unit.to_lowercase().contains(session)))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifs::store::Store;
    use zbus::zvariant::Value;

    /// The whole path a real notification takes: a `Notify` call on the bus,
    /// through the interface, into the list.
    ///
    /// Served under a name of its own rather than the real one, so that
    /// running the suite on a live desktop does not take the notification
    /// service away from whoever currently owns it.
    #[test]
    fn a_notify_call_on_the_bus_lands_in_the_list() {
        const TEST_NAME: &str = "gg.caelestia.NotifsTest";

        let notifs = Notifs::with_store(Store::nowhere());
        let Ok(connection) = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(TEST_NAME))
            .and_then(|b| b.serve_at(PATH, Server { notifs: notifs.clone() }))
            .and_then(|b| b.build())
        else {
            // No session bus: a build machine, not a desktop. Nothing to
            // prove here and nothing broken.
            return;
        };

        let proxy = zbus::blocking::Proxy::new(&connection, TEST_NAME, PATH, NAME)
            .expect("the interface is served");

        let hints: HashMap<&str, Value> =
            HashMap::from([("urgency", Value::U8(2)), ("value", Value::I32(40))]);
        let id: u32 = proxy
            .call(
                "Notify",
                &("test-sender", 0u32, "", "A summary", "A body", vec!["default", "Open"], hints, 0i32),
            )
            .expect("Notify is answered");

        assert!(id > 0, "the spec wants a non-zero id");

        let list = notifs.feed().list;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].summary, "A summary");
        assert_eq!(list[0].app_name, "test-sender");
        assert_eq!(list[0].urgency, 2, "the urgency hint was dropped");
        assert_eq!(list[0].progress, Some(40), "the value hint was dropped");
        assert_eq!(list[0].actions, vec![Action { identifier: "default".into(), text: "Open".into() }]);

        // And the capabilities a sender checks before it bothers.
        let capabilities: Vec<String> =
            proxy.call("GetCapabilities", &()).expect("GetCapabilities is answered");
        assert!(capabilities.iter().any(|c| c == "actions"));
        assert!(capabilities.iter().any(|c| c == "body"));

        // Closing it over the bus empties the list again.
        let () = proxy.call("CloseNotification", &(id,)).expect("CloseNotification is answered");
        assert!(notifs.feed().list.is_empty(), "closing over the bus left it in the list");
    }

    #[test]
    fn actions_arrive_as_alternating_id_and_label() {
        let flat =
            ["default", "Open", "reply", "Reply"].map(str::to_string).to_vec();
        let actions: Vec<Action> = flat
            .chunks_exact(2)
            .map(|pair| Action { identifier: pair[0].clone(), text: pair[1].clone() })
            .collect();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[1], Action { identifier: "reply".into(), text: "Reply".into() });

        // An odd trailing entry is a malformed sender, and must not panic or
        // invent a label.
        assert!(["default".to_string()].chunks_exact(2).next().is_none());
    }

    #[test]
    fn the_shell_and_the_session_are_never_stopped_for_the_name() {
        for comm in ["Hyprland", "qs", "quickshell", "plasmashell", "systemd", "caelestia-bar", "cae-shell"] {
            assert!(NEVER_STOPPED.contains(&comm), "{comm} could be killed for the bus name");
        }
        assert!(!NEVER_STOPPED.contains(&"dunst"));
        assert!(!NEVER_STOPPED.contains(&"mako"));
    }

    #[test]
    fn a_unit_shared_with_the_session_is_not_a_dedicated_one() {
        // This process runs in whatever the test runner's unit is, which is a
        // session or an app scope and never a `.service` of its own making —
        // the point is only that the parse survives a real cgroup file.
        let _ = dedicated_unit(std::process::id());

        let shared = "0::/user.slice/user-1000.slice/user@1000.service/session.slice/wayland-wm@hyprland.service";
        let found = shared
            .split('/')
            .filter(|part| part.ends_with(".service"))
            .find(|unit| !SESSION_UNITS.iter().any(|session| unit.to_lowercase().contains(session)));
        assert_eq!(found, None, "a session unit was offered up to be stopped");
    }
}
