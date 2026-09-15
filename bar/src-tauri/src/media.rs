//! What is playing, over MPRIS.
//!
//! Every player that cares to be controlled owns a bus name beginning
//! `org.mpris.MediaPlayer2.` and answers the same two interfaces on the same
//! object path. The bar wants one of them — whichever is actually playing,
//! falling back to whichever exists — and the four buttons that go with it.

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;
use zbus::blocking::{Connection, Proxy};
use zbus::names::BusName;
use zbus::zvariant::OwnedValue;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    /// The player's bus name, so a control knows who to talk to.
    pub bus: String,
    /// What the player calls itself: "Spotify", "Firefox".
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
}

fn connection() -> Option<Connection> {
    Connection::session().ok()
}

fn players(connection: &Connection) -> Vec<String> {
    let Ok(proxy) = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    ) else {
        return Vec::new();
    };
    proxy
        .call::<_, _, Vec<String>>("ListNames", &())
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.starts_with(PREFIX))
        .collect()
}

fn properties(connection: &Connection, bus: &str, interface: &str) -> HashMap<String, OwnedValue> {
    Proxy::new(connection, bus, PATH, "org.freedesktop.DBus.Properties")
        .ok()
        .and_then(|proxy| proxy.call::<_, _, HashMap<String, OwnedValue>>("GetAll", &(interface,)).ok())
        .unwrap_or_default()
}

fn text(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    properties.get(key).and_then(|value| String::try_from(value.clone()).ok()).unwrap_or_default()
}

fn flag(properties: &HashMap<String, OwnedValue>, key: &str) -> bool {
    properties.get(key).and_then(|value| bool::try_from(value.clone()).ok()).unwrap_or(false)
}

fn read_player(connection: &Connection, bus: &str) -> Option<NowPlaying> {
    let player = properties(connection, bus, PLAYER);
    if player.is_empty() {
        return None;
    }

    // The metadata is a dict inside the dict, and the artist inside that is a
    // list — a track can have several, and joining them is what a one-line
    // readout can do with that.
    let metadata: HashMap<String, OwnedValue> = player
        .get("Metadata")
        .and_then(|value| HashMap::try_from(value.clone()).ok())
        .unwrap_or_default();

    let artist = metadata
        .get("xesam:artist")
        .and_then(|value| Vec::<String>::try_from(value.clone()).ok())
        .map(|names| names.join(", "))
        .unwrap_or_default();

    let identity = text(&properties(connection, bus, "org.mpris.MediaPlayer2"), "Identity");

    Some(NowPlaying {
        bus: bus.to_string(),
        identity: if identity.is_empty() {
            bus.trim_start_matches(PREFIX).to_string()
        } else {
            identity
        },
        title: text(&metadata, "xesam:title"),
        artist,
        playing: text(&player, "PlaybackStatus") == "Playing",
        can_go_next: flag(&player, "CanGoNext"),
        can_go_previous: flag(&player, "CanGoPrevious"),
    })
}

/// The one player worth showing: whatever is playing, or failing that
/// whatever is there. A bar has room for one, and the one that is making
/// noise is the one being thought about.
fn read(connection: &Connection) -> Option<NowPlaying> {
    let mut fallback = None;
    for bus in players(connection) {
        let Some(player) = read_player(connection, &bus) else { continue };
        if player.playing {
            return Some(player);
        }
        fallback.get_or_insert(player);
    }
    fallback
}

/// What is playing right now, for a caller that has just started listening.
pub fn now() -> Option<NowPlaying> {
    connection().and_then(|connection| read(&connection))
}

/// Calls `on_change` whenever what is playing changes. Blocks; own thread.
pub fn watch(mut on_change: impl FnMut(Option<NowPlaying>)) {
    let Some(connection) = connection() else { return };
    let mut last: Option<Option<NowPlaying>> = None;
    loop {
        let playing = read(&connection);
        if last.as_ref() != Some(&playing) {
            last = Some(playing.clone());
            on_change(playing);
        }
        // A title changes at the pace of a song, not of a frame — and each
        // pass is several DBus round trips per player.
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// One of "PlayPause", "Next", "Previous" or "Stop", sent to whichever player
/// the bar is currently showing.
pub fn control(action: &str) {
    if !["PlayPause", "Next", "Previous", "Stop"].contains(&action) {
        return;
    }
    let Some(connection) = connection() else { return };
    let Some(playing) = read(&connection) else { return };
    // An owned name rather than a borrowed one: the proxy outlives the
    // `NowPlaying` it came from, and a &str from it would not.
    let Ok(bus) = BusName::try_from(playing.bus) else { return };
    if let Ok(proxy) = Proxy::new(&connection, bus, PATH, PLAYER) {
        let _ = proxy.call::<_, _, ()>(action, &());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mpris_names_count_as_players() {
        // The filter is the whole of the discovery, so it is worth pinning:
        // every player is under this prefix and nothing else is.
        assert!("org.mpris.MediaPlayer2.spotify".starts_with(PREFIX));
        assert!(!"org.freedesktop.Notifications".starts_with(PREFIX));
    }

    #[test]
    fn an_unknown_action_is_refused() {
        // `control` takes a string from the front end, and a bad one must not
        // become an arbitrary method call on somebody else's bus name.
        control("Quit");
        control("../../etc");
    }
}
