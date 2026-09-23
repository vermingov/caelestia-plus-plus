//! Being told over D-Bus that something changed, rather than asking.
//!
//! The players and the tray's items all say when anything about them
//! changes. Asking them instead, on a tick, woke every one of them to answer
//! whether or not anything had — a browser every second, each tray
//! application every four — for the whole of a session.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use zbus::blocking::Connection;

/// A channel that has something in it whenever a signal matching one of
/// `rules` arrives on `connection`, from a thread that listens for good; and
/// a sender into it, for waking the same waiter by hand.
pub fn listen(connection: &Connection, rules: &[&str], name: &str) -> Option<(Sender<()>, Receiver<()>)> {
    let bus = zbus::blocking::fdo::DBusProxy::new(connection).ok()?;
    for rule in rules {
        bus.add_match_rule(zbus::MatchRule::try_from(*rule).ok()?).ok()?;
    }
    let messages = zbus::blocking::MessageIterator::from(connection.clone());
    let (told, heard) = std::sync::mpsc::channel();
    let telling = told.clone();
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            // The connection's replies to the asking come through here too;
            // only what is said unasked means anything changed.
            let said = messages.filter_map(Result::ok).filter(|message| message.message_type() == zbus::message::Type::Signal);
            for _ in said {
                if telling.send(()).is_err() {
                    return;
                }
            }
        })
        .ok()?;
    Some((told, heard))
}

/// Waits to be told something changed — for as long as `patience` at most,
/// after which it is time to look anyway — and then for the rest of what is
/// said with it, which is usually several signals at once. Without a
/// listener, it is the tick it replaced.
pub fn wait(heard: Option<&Receiver<()>>, patience: Duration, settle: Duration, tick: Duration) {
    let Some(heard) = heard else { return std::thread::sleep(tick) };
    match heard.recv_timeout(patience) {
        Ok(()) => settle_down(heard, settle),
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => std::thread::sleep(tick),
    }
}

/// Takes in the rest of a burst: until nothing has been said for `settle`,
/// but never for longer than a few of those in all. An application that
/// never stops talking — a tray icon that animates, say — is still looked
/// at, rather than waited on for as long as it goes on.
fn settle_down(heard: &Receiver<()>, settle: Duration) {
    let enough = Instant::now() + settle * 4;
    loop {
        let left = enough.saturating_duration_since(Instant::now());
        if left.is_zero() || heard.recv_timeout(settle.min(left)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_is_taken_in_whole() {
        let (told, heard) = std::sync::mpsc::channel();
        for _ in 0..5 {
            told.send(()).unwrap();
        }
        settle_down(&heard, Duration::from_millis(20));
        assert!(heard.try_recv().is_err(), "all of it, in one");
    }

    #[test]
    fn a_voice_that_never_stops_is_still_answered() {
        let (told, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while told.send(()).is_ok() {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let started = Instant::now();
        settle_down(&heard, Duration::from_millis(20));
        assert!(started.elapsed() < Duration::from_millis(200), "waited {:?}", started.elapsed());
    }
}
