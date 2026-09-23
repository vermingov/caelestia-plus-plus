//! The shell's door: a socket anything on the desktop can knock on.
//!
//! The launcher and the notifications each have one of their own, because
//! they took over from programs that did, and what talks to them already
//! knows where they live. Everything else the shell can be asked for comes
//! through here, a line at a time:
//!
//!   settings [PAGE]    open the settings, on a page if one is named
//!   dashboard [show|hide|toggle]
//!   session            the session menu, or away with it if it is up
//!   utilities [show|hide|toggle]
//!   osd [show|hide|toggle]
//!   picker [freeze] [clip]    choose a piece of the screen to shoot
//!   security [overview|protection|firewall|startup]
//!   features                  the machine's modes, or away with them
//!   lock                      the session, until a password says otherwise
//!   unlock                    it again, for whatever knows the password
//!   launcher [show|hide|toggle] [QUERY]
//!   centre [show|hide|toggle]  the notifications, on the focused screen
//!   notifs clear               every notification away
//!   showall                    everything a reach opens, at once
//!   brightness up|down         what the keys on a laptop do
//!   volume up|down|mute
//!   microphone mute
//!   media play|next|previous|stop
//!
//! `cae-shell settings network` is the same knock from a command line: run
//! with a verb, this program says it to the shell that is running and exits.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::App;

use crate::actions;
use crate::ui::{Ask, dashboard, eggs, features, launcher, lock, osd, picker, security, session, settings, utilities};

/// What can be said through it.
const VERBS: [&str; 21] = [
    "settings",
    "dashboard",
    "session",
    "utilities",
    "osd",
    "picker",
    "security",
    "features",
    "launcher",
    "centre",
    "notifs",
    "showall",
    "brightness",
    "volume",
    "microphone",
    "media",
    "lock",
    "unlock",
    "scan",
    "egg",
    "cinema",
];

fn path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    runtime.join("caelestia-shell.sock")
}

/// Whether these are words for a running shell rather than a shell to start.
pub fn is_knock(words: &[String]) -> bool {
    words.first().is_some_and(|verb| VERBS.contains(&verb.as_str()))
}

/// Says `words` to the shell that is running. False when none is.
pub fn knock(words: &[String]) -> bool {
    let Ok(mut door) = UnixStream::connect(path()) else { return false };
    door.write_all(format!("{}\n", words.join(" ")).as_bytes()).is_ok()
}

/// Starts answering, unless a shell already is: the door belongs to the one
/// in use, and one started beside it to be looked at must not take it. Says
/// whether this shell is the one that answers, which is also whether it is
/// the one that should be opening things of its own accord.
pub fn open(cx: &mut App) -> bool {
    if UnixStream::connect(path()).is_ok() {
        return false;
    }
    let (said, mut heard) = unbounded::<String>();
    std::thread::spawn(move || listen(said));

    cx.spawn(async move |cx| {
        while let Some(line) = heard.next().await {
            cx.update(|cx| answer(&line, cx));
        }
    })
    .detach();
    true
}

fn answer(line: &str, cx: &mut App) {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("settings") => settings::open(words.next(), cx),
        Some("dashboard") => dashboard::ask(Ask::named(words.next()), cx),
        Some("session") => session::ask(Ask::named(words.next()), cx),
        Some("utilities") => utilities::answer(Ask::named(words.next()), cx),
        Some("osd") => osd::ask(Ask::named(words.next()), cx),
        Some("picker") => picker::ask(picker::Want::from(&mut words), cx),
        Some("security") => security::ask(security::Tab::named(words.next()), cx),
        Some("features") => features::toggle(cx),
        Some("lock") => {
            let feeds = cx.global::<crate::feeds::Feeds>().clone();
            lock::lock(cx, &feeds);
        }
        Some("unlock") => lock::unlock(cx),
        // Looking over the machine by hand: the page when it is asked for,
        // the prompt when the scan is what was asked for.
        Some("egg") => eggs::pop(cx),
        Some("cinema") => eggs::cinema::pop(cx),
        Some("scan") => match words.next() {
            Some("now") => crate::setup::look(cae_core::checkup::Pace::Asked, cx),
            _ => settings::open(Some("scan"), cx),
        },
        Some("launcher") => drop(launcher::ask(words.next(), words.collect::<Vec<_>>().join(" "), cx)),
        Some("centre") => actions::toggle_centre_here(Ask::named(words.next()), cx),
        Some("notifs") if words.next() == Some("clear") => actions::clear_notifications(cx),
        // Everything a reach into a corner would open, for the one key that
        // asks for all of it.
        Some("showall") => {
            dashboard::ask(Ask::Toggle, cx);
            osd::ask(Ask::Toggle, cx);
            utilities::answer(Ask::Toggle, cx);
            launcher::ask(Some("toggle"), String::new(), cx);
        }
        Some("brightness") => actions::brightness(cx, words.next() != Some("down")),
        Some("volume") => match words.next() {
            Some("mute") => actions::mute(cx),
            word => actions::volume(cx, word != Some("down")),
        },
        Some("microphone") => actions::mute_microphone(cx),
        Some("media") => actions::media(cx, match words.next() {
            Some("next") => "next",
            Some("previous") | Some("prev") => "previous",
            Some("stop") => "stop",
            _ => "playPause",
        }),
        _ => eprintln!("cae: nothing here answers to {line:?}"),
    }
}

fn listen(said: UnboundedSender<String>) {
    let path = path();
    // Left behind by a shell that was killed. Nothing is listening on it:
    // that was checked before this was called.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => return eprintln!("cae: cannot listen on {}: {error}", path.display()),
    };
    for stream in listener.incoming().flatten() {
        let said = said.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if said.unbounded_send(line).is_err() {
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn every_verb_the_door_answers_is_one_a_command_line_may_say() {
        for verb in VERBS {
            assert!(is_knock(&words(verb)), "{verb} is a verb but not a knock");
        }
        assert!(is_knock(&words("volume up")), "a verb with a word after it is still a knock");
        assert!(!is_knock(&words("--preview")), "a flag is how a shell is started, not a knock");
        assert!(!is_knock(&[]), "nothing said is a shell to start");
    }
}
