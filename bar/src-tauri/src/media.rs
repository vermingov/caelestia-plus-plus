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
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

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
    pub album: String,
    /// Where the cover is: a `file://` of the player's own, or an address on
    /// the web. Empty where the player gives none.
    pub art: String,
    /// How long the track is, in microseconds. Zero for a stream, which has
    /// no end to be some way towards.
    pub length: i64,
    /// The player's own name for the track, which seeking has to quote back
    /// at it so that a seek never lands in the next song.
    pub track: String,
    pub playing: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
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

/// A count of microseconds. The specification says signed; players that did
/// not read it send unsigned, and one or two send a double.
fn whole(properties: &HashMap<String, OwnedValue>, key: &str) -> i64 {
    let Some(value) = properties.get(key) else { return 0 };
    i64::try_from(value.clone())
        .ok()
        .or_else(|| u64::try_from(value.clone()).ok().map(|length| length as i64))
        .or_else(|| f64::try_from(value.clone()).ok().map(|length| length as i64))
        .unwrap_or(0)
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
        album: text(&metadata, "xesam:album"),
        art: text(&metadata, "mpris:artUrl"),
        length: whole(&metadata, "mpris:length"),
        track: metadata
            .get("mpris:trackid")
            .and_then(|value| OwnedObjectPath::try_from(value.clone()).ok())
            .map(|path| path.as_str().to_string())
            .unwrap_or_else(|| text(&metadata, "mpris:trackid")),
        playing: text(&player, "PlaybackStatus") == "Playing",
        can_go_next: flag(&player, "CanGoNext"),
        can_go_previous: flag(&player, "CanGoPrevious"),
        can_seek: flag(&player, "CanSeek"),
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

/// Every player there is, the ones that are playing first: for somewhere
/// with room to choose between them.
#[cfg_attr(feature = "tauri-ui", allow(dead_code))]
pub fn all() -> Vec<NowPlaying> {
    let Some(connection) = connection() else { return Vec::new() };
    let mut players: Vec<NowPlaying> =
        players(&connection).iter().filter_map(|bus| read_player(&connection, bus)).collect();
    players.sort_by_key(|player| !player.playing);
    players
}

/// How far into its track a player is, in microseconds. Asked for rather
/// than watched: a player does not announce its position as it plays, only
/// when it jumps.
#[cfg_attr(feature = "tauri-ui", allow(dead_code))]
pub fn position(bus: &str) -> Option<i64> {
    let connection = connection()?;
    let proxy = Proxy::new(&connection, BusName::try_from(bus.to_string()).ok()?, PATH, PLAYER).ok()?;
    proxy.get_property::<i64>("Position").ok()
}

/// Moves a player to `position` microseconds into `track`, which is the
/// track it was showing when somebody chose where to go.
#[cfg_attr(feature = "tauri-ui", allow(dead_code))]
pub fn set_position(bus: &str, track: &str, position: i64) {
    let Some(connection) = connection() else { return };
    let (Ok(bus), Ok(track)) = (BusName::try_from(bus.to_string()), ObjectPath::try_from(track.to_string())) else { return };
    if let Ok(proxy) = Proxy::new(&connection, bus, PATH, PLAYER) {
        let _ = proxy.call::<_, _, ()>("SetPosition", &(track, position));
    }
    nudge();
}

/// `control`, for one player in particular.
#[cfg_attr(feature = "tauri-ui", allow(dead_code))]
pub fn control_on(bus: &str, action: &str) {
    if !["PlayPause", "Next", "Previous", "Stop"].contains(&action) {
        return;
    }
    let Some(connection) = connection() else { return };
    let Ok(bus) = BusName::try_from(bus.to_string()) else { return };
    if let Ok(proxy) = Proxy::new(&connection, bus, PATH, PLAYER) {
        let _ = proxy.call::<_, _, ()>(action, &());
    }
    nudge();
}

/// What is playing right now, for a caller that has just started listening.
pub fn now() -> Option<NowPlaying> {
    connection().and_then(|connection| read(&connection))
}

/// What players say when anything about them changes, and what the bus says
/// when one comes or goes.
const SAID: [&str; 2] = [
    "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'",
    "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'",
];

/// A new track is several things said at once — the metadata, then that it
/// is playing — and they are read as one.
const SETTLE: Duration = Duration::from_millis(80);

/// How long the players are trusted to say what changed before it is looked
/// at anyway. The specification has every player announce a new track or a
/// pause; one that does not is shown it this late at worst.
const LOOK_ANYWAY: Duration = Duration::from_secs(30);

/// Calls `on_change` whenever what is playing changes. Blocks; own thread.
///
/// Told by the players rather than asking them. Asking was a round of calls
/// every two seconds to every player there was — a browser woken to answer,
/// all day, whether anything was playing or not.
pub fn watch(mut on_change: impl FnMut(Option<NowPlaying>)) {
    let Some(connection) = connection() else { return };
    let heard = crate::signals::listen(&connection, &SAID, "mpris").map(|(told, heard)| {
        let _ = NUDGE.set(told);
        heard
    });
    let mut last: Option<Option<NowPlaying>> = None;
    loop {
        let playing = read(&connection);
        if last.as_ref() != Some(&playing) {
            last = Some(playing.clone());
            on_change(playing);
        }
        crate::signals::wait(heard.as_ref(), LOOK_ANYWAY, SETTLE, Duration::from_secs(2));
    }
}

/// Wakes `watch` when this process has just told a player to do something:
/// what it did is shown at once, whether or not the player says so itself.
static NUDGE: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

fn nudge() {
    if let Some(nudge) = NUDGE.get() {
        let _ = nudge.send(());
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
    nudge();
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
