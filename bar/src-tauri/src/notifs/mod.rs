//! The desktop's notification server.
//!
//! This used to be the shell's: `services/Notifs.qml` owned
//! `org.freedesktop.Notifications` and every piece of notification UI was
//! QML. The bar draws them now, and a bar that asked the shell for its
//! notifications would be a webview waiting on a QML runtime for something it
//! is perfectly able to receive itself — the same reasoning as `guards` and
//! `tray`, which read their own sockets and their own bus.
//!
//! What lives in this file is the state and every rule about it: the history,
//! do-not-disturb, which notifications get to interrupt and for how long. The
//! rest is in the modules beside it — `bus` speaks D-Bus, `store` keeps the
//! history on disk, `bridge` tells the shell's lock screen what there is, and
//! `window` owns the surfaces the toasts are drawn on. None of them draw
//! anything; the front end is handed a list and told when it changes.

mod bridge;
mod bus;
pub mod commands;
mod images;
mod store;
pub mod window;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How many are kept. The shell settled on this number after restoring a
/// thousand froze it for seconds on startup and every save rewrote hundreds
/// of kilobytes; nothing about moving the code changes that arithmetic.
const MAX_HISTORY: usize = 300;

/// Why a notification went away, as the spec numbers it. Senders watch this:
/// a mail client wants to know whether you dismissed its alert or it simply
/// timed out.
pub mod reason {
    pub const DISMISSED: u32 = 2;
    pub const CLOSED_BY_CALL: u32 = 3;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub identifier: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notification {
    pub id: u32,
    /// Milliseconds since the epoch. The front end does the "3m ago" wording,
    /// because that is a thing that changes while nothing else does.
    ///
    /// Read with `store::epoch_ms`, which also accepts the ISO-8601 string
    /// the QML server wrote — the history on disk was written by that server,
    /// and a bar that could not read it would throw away three hundred
    /// notifications the first time it started.
    #[serde(deserialize_with = "store::epoch_ms")]
    pub time: i64,
    pub app_name: String,
    /// A path or a data URI the webview can load; never a bare theme name,
    /// which it has no way to resolve.
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    /// The picture the notification carried, if it carried one — album art,
    /// a contact photo, a screenshot.
    pub image: String,
    /// 0 low, 1 normal, 2 critical.
    pub urgency: u8,
    /// The sender wants it to survive being acted on.
    pub resident: bool,
    /// The sender does not want it kept in the history at all.
    pub transient: bool,
    pub has_action_icons: bool,
    /// What the sender asked for, in milliseconds: -1 means "you decide",
    /// 0 means "never expire".
    pub expire_timeout: i32,
    pub actions: Vec<Action>,
    /// 0–100 from the `value` hint: a download, a copy, a volume change. Drawn
    /// as a ring around the icon.
    pub progress: Option<i32>,
    /// Whether it is on screen as a toast right now. False for everything
    /// restored from disk, and for anything that arrived while do not disturb
    /// was on.
    pub popup: bool,
}

/// What the front end is given: the list, and the two switches that change
/// how the list behaves.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    pub list: Vec<Notification>,
    pub dnd: bool,
    /// The output the notification centre is open on; empty while it is shut.
    /// One of them at most: it follows the person, not the desktop.
    pub centre: String,
    /// How many have arrived since the centre was last open.
    pub unseen: usize,
}

/// What a bar needs for its bell, and no more.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// What the bell's badge says. Not the length of the history: that is
    /// three hundred on a machine that never clears it, and a badge that
    /// always says "99+" is a badge that says nothing.
    pub unseen: usize,
    pub dnd: bool,
    pub centre: String,
}

impl Summary {
    pub fn of(feed: &Feed) -> Summary {
        Summary { unseen: feed.unseen, dnd: feed.dnd, centre: feed.centre.clone() }
    }
}

/// Everything the server knows, behind one lock.
#[derive(Default)]
struct State {
    /// Newest first, which is the order both the toasts and the list want.
    list: Vec<Notification>,
    dnd: bool,
    centre: String,
    /// Arrived since the centre was last open. Everything restored from disk
    /// counts as seen: it was there before this session began.
    unseen: usize,
    /// The last id handed out. The spec wants them non-zero and unique for
    /// the life of the server.
    last_id: u32,
    /// One counter per toast, bumped whenever its clock is restarted or
    /// stopped. A timer thread that wakes to find a different number has been
    /// superseded — the pointer came to rest on the toast, or it was replaced
    /// — and does nothing.
    clocks: HashMap<u32, u64>,
}

impl State {
    fn feed(&self) -> Feed {
        Feed {
            list: self.list.clone(),
            dnd: self.dnd,
            centre: self.centre.clone(),
            // Never more than there are: closing them one by one from a toast
            // does not open the centre, and must not leave a count of ghosts.
            unseen: self.unseen.min(self.list.len()),
        }
    }
}

type Listener = Box<dyn Fn(&Feed) + Send + 'static>;

/// The server, as everything outside this module holds it.
#[derive(Clone)]
pub struct Notifs {
    state: Arc<Mutex<State>>,
    /// Where the history is kept. A field rather than a constant because a
    /// test that wrote to the real one would throw away the notifications on
    /// the machine it ran on — which is exactly what it did before this was
    /// a field.
    store: store::Store,
    /// Told whenever anything changes: the windows, and the shell's bridge.
    listeners: Arc<Mutex<Vec<Listener>>>,
    /// Held so a click on a toast can answer the sender.
    bus: Arc<Mutex<Option<zbus::blocking::Connection>>>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}

/// Whether the person is looking at something fullscreen, and what the config
/// says to do about that. Asked once per arriving notification, before the
/// state lock is taken: it is a compositor round trip, and the lock is not a
/// thing to hold across one.
struct Moment {
    fullscreen: bool,
    config: crate::logo::NotifsConfig,
}

impl Moment {
    fn now() -> Moment {
        Moment { fullscreen: crate::hypr::fullscreen_focused(), config: crate::logo::notifs_config() }
    }

    /// How long a toast stays on screen, or None if it should stay until it
    /// is dealt with.
    ///
    /// `expire_timeout` is the sender's wish in milliseconds: 0 means never,
    /// -1 means the server decides. A critical notification overrides both —
    /// the spec is explicit that those are not to be taken away on a timer.
    fn lifetime(&self, notif: &Notification) -> Option<Duration> {
        if !notif.popup || notif.urgency >= 2 {
            return None;
        }
        let ms = match notif.expire_timeout {
            0 => return None,
            timeout if timeout > 0 => timeout as u64,
            _ if self.fullscreen => self.config.fullscreen_expire_timeout,
            _ => self.config.default_expire_timeout,
        };
        Some(Duration::from_millis(ms))
    }
}

impl Notifs {
    pub fn new() -> Notifs {
        Notifs::with_store(store::Store::xdg())
    }

    fn with_store(store: store::Store) -> Notifs {
        let stored = store.load();
        let state = State {
            // Ids restored from disk must not be handed out again to a live
            // notification, or closing the new one would close the old.
            last_id: stored.iter().map(|n| n.id).max().unwrap_or(0),
            dnd: store.load_dnd(),
            list: stored,
            ..State::default()
        };
        Notifs {
            state: Arc::new(Mutex::new(state)),
            store,
            listeners: Arc::new(Mutex::new(Vec::new())),
            bus: Arc::new(Mutex::new(None)),
        }
    }

    /// Adds somewhere for change notifications to go.
    pub fn on_change(&self, listener: impl Fn(&Feed) + Send + 'static) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.push(Box::new(listener));
        }
    }

    pub fn feed(&self) -> Feed {
        self.state.lock().map(|state| state.feed()).unwrap_or_default()
    }

    /// Runs one change against the state and tells everybody about it.
    ///
    /// `change` answers whether it changed anything. The feed is built under
    /// the same lock the change was made under, so a change and the news of
    /// it cannot interleave with another change.
    fn update(&self, change: impl FnOnce(&mut State) -> bool) {
        let feed = {
            let Ok(mut state) = self.state.lock() else { return };
            if !change(&mut state) {
                return;
            }
            state.feed()
        };
        self.store.save(&feed.list);
        if let Ok(listeners) = self.listeners.lock() {
            for listener in listeners.iter() {
                listener(&feed);
            }
        }
    }

    pub fn set_dnd(&self, on: bool) {
        self.store.save_dnd(on);
        self.update(|state| {
            state.dnd = on;
            // Turning it on clears what is already on screen: the point of
            // the switch is to stop being interrupted, and leaving three
            // toasts up would be a strange way to honour that.
            if on {
                state.list.iter_mut().for_each(|notif| notif.popup = false);
            }
            true
        });
    }

    pub fn toggle_dnd(&self) {
        let on = self.state.lock().map(|state| state.dnd).unwrap_or(false);
        self.set_dnd(!on);
    }

    /// Opens the centre on one output, or shuts it with an empty name.
    ///
    /// Opening takes every toast down with it: they are all in the list that
    /// has just opened, and a notification shown twice on one screen is
    /// noise. While it is open nothing new pops up either, for the same
    /// reason.
    pub fn set_centre(&self, output: &str) {
        self.update(|state| {
            if state.centre == output {
                return false;
            }
            state.centre = output.to_string();
            if !output.is_empty() {
                state.list.iter_mut().for_each(|notif| notif.popup = false);
                state.unseen = 0;
            }
            true
        });
    }

    pub fn toggle_centre(&self, output: &str) {
        let open = self.state.lock().map(|state| !state.centre.is_empty()).unwrap_or(false);
        self.set_centre(if open { "" } else { output });
    }

    /// Takes one off the screen without taking it out of the history.
    pub fn dismiss_popup(&self, id: u32) {
        self.update(|state| {
            state.clocks.remove(&id);
            match state.list.iter_mut().find(|n| n.id == id && n.popup) {
                Some(notif) => {
                    notif.popup = false;
                    true
                }
                None => false,
            }
        });
    }

    /// Closes one for good, and tells whoever sent it.
    pub fn close(&self, id: u32, why: u32) {
        let mut closed = false;
        self.update(|state| {
            let before = state.list.len();
            state.list.retain(|n| n.id != id);
            state.clocks.remove(&id);
            closed = state.list.len() != before;
            closed
        });
        if closed {
            self.emit_closed(id, why);
        }
    }

    /// Closes everything one application sent — a swipe on its group.
    pub fn close_app(&self, app_name: &str) {
        let mut closed = Vec::new();
        self.update(|state| {
            closed = state.list.iter().filter(|n| n.app_name == app_name).map(|n| n.id).collect();
            state.list.retain(|n| n.app_name != app_name);
            !closed.is_empty()
        });
        for id in closed {
            self.emit_closed(id, reason::DISMISSED);
        }
    }

    pub fn clear(&self) {
        let mut closed = Vec::new();
        self.update(|state| {
            closed = state.list.drain(..).map(|n| n.id).collect();
            state.clocks.clear();
            state.unseen = 0;
            !closed.is_empty()
        });
        for id in closed {
            self.emit_closed(id, reason::DISMISSED);
        }
    }

    /// Tells the sender a button was pressed.
    ///
    /// A notification that is not `resident` is finished once it has been
    /// acted on — that is what the spec says the flag means — so it goes as
    /// well, and the sender is told why.
    pub fn invoke(&self, id: u32, action: &str) {
        let resident = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.list.iter().find(|n| n.id == id).map(|n| n.resident))
            .unwrap_or(false);

        self.emit("ActionInvoked", &(id, action));

        if resident {
            self.dismiss_popup(id);
        } else {
            self.close(id, reason::DISMISSED);
        }
    }

    fn emit_closed(&self, id: u32, why: u32) {
        self.emit("NotificationClosed", &(id, why));
    }

    fn emit<B>(&self, signal: &str, body: &B)
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let Ok(bus) = self.bus.lock() else { return };
        let Some(connection) = bus.as_ref() else { return };
        let _ = connection.emit_signal(None::<()>, bus::PATH, bus::NAME, signal, body);
    }

    /// Takes one notification from the bus into the history.
    fn accept(&self, mut notif: Notification, replaces: u32) -> u32 {
        let moment = Moment::now();
        let mut id = 0;

        self.update(|state| {
            // Replacing keeps the id, which is how a progress notification
            // updates in place instead of stacking up.
            id = if replaces != 0 && state.list.iter().any(|n| n.id == replaces) {
                state.list.retain(|n| n.id != replaces);
                replaces
            } else {
                state.last_id = state.last_id.wrapping_add(1).max(1);
                state.last_id
            };
            notif.id = id;
            notif.popup = !state.dnd
                && state.centre.is_empty()
                && (moment.config.fullscreen || !moment.fullscreen);
            // Replacing one that is already there is not a new arrival, and
            // nothing is unseen while the list is open in front of somebody.
            if replaces != id && state.centre.is_empty() {
                state.unseen += 1;
            }

            state.list.insert(0, notif.clone());
            // Anything the sender marked transient is shown and forgotten;
            // it never belonged in a history.
            state.list.retain(|n| !n.transient || n.id == id);
            state.list.truncate(MAX_HISTORY);
            true
        });

        self.start_clock(id, &moment);
        id
    }

    /// Starts the countdown that takes a toast off the screen.
    ///
    /// A thread rather than a timer wheel: notifications arrive a few a
    /// minute, not a few a millisecond.
    fn start_clock(&self, id: u32, moment: &Moment) {
        let Ok(mut state) = self.state.lock() else { return };
        let Some(after) = state.list.iter().find(|n| n.id == id).and_then(|n| moment.lifetime(n)) else {
            return;
        };
        let clock = state.clocks.entry(id).or_insert(0);
        *clock += 1;
        let started = *clock;
        drop(state);

        let notifs = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            notifs.expire(id, started);
        });
    }

    /// Stops the countdown: the pointer has come to rest on the toast, and
    /// taking away something that is being read is rude.
    pub fn hold(&self, id: u32) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(clock) = state.clocks.get_mut(&id) {
                *clock += 1;
            }
        }
    }

    /// Starts it again from the top once the pointer has gone, which is what
    /// the shell's own did: a full interval, not whatever was left of one.
    pub fn release(&self, id: u32) {
        self.start_clock(id, &Moment::now());
    }

    /// The end of a toast's time on screen — unless its clock was restarted
    /// or stopped while this thread slept.
    ///
    /// The toast goes; the notification does not. It drops into the list and
    /// waits to be read, which is what the shell's server did and why a
    /// history exists at all. With `expire` off a toast outstays its clock,
    /// except over a fullscreen window, where nothing may sit for ever.
    fn expire(&self, id: u32, started: u64) {
        let current = self.state.lock().ok().and_then(|state| state.clocks.get(&id).copied());
        if current != Some(started) {
            return;
        }
        if crate::logo::notifs_config().expire || crate::hypr::fullscreen_focused() {
            self.dismiss_popup(id);
        }
    }
}

/// Brings the server up: the bus name, and the socket the shell listens on.
///
/// Returns the handle immediately; the bus work happens on its own thread,
/// because taking the name can block on another daemon being asked to let go
/// of it.
pub fn start() -> Notifs {
    let notifs = Notifs::new();
    notifs.update(|state| {
        state.list.iter_mut().for_each(images::normalise);
        true
    });

    let pictures: Vec<String> =
        notifs.feed().list.into_iter().flat_map(|n| [n.image, n.app_icon]).filter(|p| !p.is_empty()).collect();
    images::prune(&pictures);

    bus::serve(notifs.clone());
    bridge::listen(notifs.clone());
    notifs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Notifs {
        Notifs::with_store(store::Store::nowhere())
    }

    fn sample(summary: &str) -> Notification {
        Notification {
            time: now_ms(),
            app_name: "test".into(),
            summary: summary.into(),
            expire_timeout: 0,
            ..Notification::default()
        }
    }

    #[test]
    fn replacing_keeps_the_id_and_the_place() {
        let notifs = server();
        let first = notifs.accept(sample("first"), 0);
        let second = notifs.accept(sample("second"), 0);
        assert_ne!(first, second);

        let again = notifs.accept(sample("first, updated"), first);
        assert_eq!(again, first, "replacing handed out a new id");

        let list = notifs.feed().list;
        assert_eq!(list.len(), 2, "replacing stacked instead of replacing");
        assert_eq!(list[0].summary, "first, updated");
    }

    #[test]
    fn a_transient_notification_is_not_kept() {
        let notifs = server();
        let mut passing = sample("passing");
        passing.transient = true;
        notifs.accept(passing, 0);
        notifs.accept(sample("kept"), 0);

        let list = notifs.feed().list;
        assert_eq!(list.len(), 1, "a transient notification stayed in the history");
        assert_eq!(list[0].summary, "kept");
    }

    #[test]
    fn do_not_disturb_takes_what_is_up_off_the_screen() {
        let notifs = server();
        notifs.accept(sample("hello"), 0);
        notifs.set_dnd(true);
        assert!(notifs.feed().list.iter().all(|n| !n.popup), "a toast survived do not disturb");

        // And nothing new gets to interrupt while it is on.
        notifs.accept(sample("quiet"), 0);
        assert!(notifs.feed().list.iter().all(|n| !n.popup));
    }

    #[test]
    fn an_open_centre_swallows_the_toasts() {
        let notifs = server();
        notifs.accept(sample("before"), 0);
        notifs.set_centre("eDP-1");
        assert!(notifs.feed().list.iter().all(|n| !n.popup), "opening the centre left a toast up");

        notifs.accept(sample("during"), 0);
        assert!(notifs.feed().list.iter().all(|n| !n.popup), "a toast appeared beside the open centre");

        notifs.set_centre("");
        let id = notifs.accept(sample("after"), 0);
        let list = notifs.feed().list;
        assert!(list.iter().find(|n| n.id == id).is_some_and(|n| n.popup));
    }

    #[test]
    fn the_bell_counts_what_arrived_since_the_centre_was_last_open() {
        let notifs = server();
        let first = notifs.accept(sample("one"), 0);
        notifs.accept(sample("two"), 0);
        assert_eq!(notifs.feed().unseen, 2);

        // An update to one that is already there is not another arrival.
        notifs.accept(sample("one, again"), first);
        assert_eq!(notifs.feed().unseen, 2);

        notifs.set_centre("eDP-1");
        assert_eq!(notifs.feed().unseen, 0, "looking at them did not count as seeing them");

        notifs.accept(sample("while it is open"), 0);
        assert_eq!(notifs.feed().unseen, 0);

        notifs.set_centre("");
        notifs.accept(sample("after"), 0);
        assert_eq!(notifs.feed().unseen, 1);

        // And never more than exist.
        notifs.clear();
        assert_eq!(notifs.feed().unseen, 0);
    }

    #[test]
    fn the_history_is_capped() {
        let notifs = server();
        for i in 0..MAX_HISTORY + 20 {
            notifs.accept(sample(&format!("notification {i}")), 0);
        }
        assert_eq!(notifs.feed().list.len(), MAX_HISTORY);
    }

    #[test]
    fn closing_an_application_takes_all_of_its_notifications() {
        let notifs = server();
        notifs.accept(sample("one"), 0);
        notifs.accept(sample("two"), 0);
        let mut other = sample("theirs");
        other.app_name = "someone else".into();
        notifs.accept(other, 0);

        notifs.close_app("test");
        let list = notifs.feed().list;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].app_name, "someone else");
    }

    #[test]
    fn a_critical_notification_is_never_taken_away_on_a_timer() {
        let moment = Moment { fullscreen: false, config: crate::logo::NotifsConfig::default() };

        let mut critical = sample("the building is on fire");
        critical.urgency = 2;
        critical.popup = true;
        critical.expire_timeout = 1000;
        assert!(moment.lifetime(&critical).is_none());

        let mut ordinary = sample("a thing happened");
        ordinary.popup = true;
        ordinary.expire_timeout = 1000;
        assert_eq!(moment.lifetime(&ordinary), Some(Duration::from_millis(1000)));

        // Zero is the sender asking for it to stay until it is dealt with.
        let mut sticky = sample("waiting on you");
        sticky.popup = true;
        sticky.expire_timeout = 0;
        assert!(moment.lifetime(&sticky).is_none());

        // And when the sender leaves it to the server, fullscreen is shorter.
        let mut unspecified = sample("whenever");
        unspecified.popup = true;
        unspecified.expire_timeout = -1;
        let watching = Moment { fullscreen: true, config: crate::logo::NotifsConfig::default() };
        assert!(watching.lifetime(&unspecified) < moment.lifetime(&unspecified));
    }

    #[test]
    fn a_held_toast_outlives_its_timer() {
        let notifs = server();
        let mut brief = sample("brief");
        brief.expire_timeout = 60;
        let id = notifs.accept(brief, 0);
        // `accept` decides `popup` from the compositor, which a test machine
        // may not have; what is under test is the clock.
        notifs.update(|state| {
            state.list.iter_mut().for_each(|n| n.popup = true);
            true
        });
        notifs.start_clock(id, &Moment { fullscreen: false, config: crate::logo::NotifsConfig::default() });

        notifs.hold(id);
        std::thread::sleep(Duration::from_millis(160));
        assert!(
            notifs.feed().list.iter().any(|n| n.id == id && n.popup),
            "a toast under the pointer was taken away"
        );

        notifs.release(id);
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            notifs.feed().list.iter().any(|n| n.id == id && !n.popup),
            "a released toast never left the screen"
        );
    }
}
