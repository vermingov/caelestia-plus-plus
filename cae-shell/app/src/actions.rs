//! What cae does to the desktop.
//!
//! Every one of these asks something outside the process: the compositor, a
//! player on the bus, the old shell while it is still around. None of them
//! may run on the thread that draws, where a slow answer is a frozen bar. They
//! are all handed to the background executor, and none has a result anything
//! waits for; what they change comes back the way everything else does,
//! through a feed.

use std::io::Write;
use std::os::unix::net::UnixStream;

use cae_core::{hypr, levels, media, notifs, system, volume};
use gpui::{App, AppContext, Global};

/// The notification server, where anything that acts on a notification can
/// find it, and whether it is this shell's to command. A shell that is only
/// being looked at beside the one in use has a server that serves nobody, and
/// whatever it is asked to do is passed to the real one instead.
pub struct Desk {
    pub server: notifs::Notifs,
    pub serving: bool,
}

impl Global for Desk {}

fn off_thread(cx: &App, work: impl FnOnce() + Send + 'static) {
    cx.background_spawn(async move { work() }).detach();
}

pub fn focus_workspace(cx: &App, id: i64) {
    off_thread(cx, move || hypr::dispatch(&format!("workspace {id}")));
}

pub fn cycle_workspace(cx: &App, forward: bool) {
    off_thread(cx, move || hypr::dispatch(if forward { "workspace r+1" } else { "workspace r-1" }));
}

pub fn toggle_special(cx: &App, name: String) {
    off_thread(cx, move || hypr::toggle_special(&name));
}

pub fn media(cx: &App, action: &'static str) {
    off_thread(cx, move || media::control(action));
}

/// One notch of a wheel, which goes as far as the settings say a notch goes.
pub fn volume(cx: &App, up: bool) {
    off_thread(cx, move || {
        let steps = levels::Steps::read();
        volume::nudge(if up { steps.volume } else { -steps.volume }, steps.loudest);
    });
}

pub fn mute(cx: &App) {
    off_thread(cx, volume::toggle_mute);
}

pub fn mute_microphone(cx: &App) {
    off_thread(cx, volume::toggle_microphone);
}

pub fn microphone(cx: &App, up: bool) {
    off_thread(cx, move || {
        let step = levels::Steps::read().volume;
        volume::nudge_microphone(if up { step } else { -step });
    });
}

pub fn brightness(cx: &App, up: bool) {
    off_thread(cx, move || {
        let step = levels::Steps::read().brightness;
        system::nudge_brightness(if up { step } else { -step });
    });
}

/// The security centre, on the page that is asking to be looked at:
/// something waiting for a verdict is a question about the firewall, and the
/// rest is the overview.
pub fn security(cx: &mut App, something_waiting: bool) {
    let tab = if something_waiting { crate::ui::security::Tab::Firewall } else { crate::ui::security::Tab::Overview };
    crate::ui::security::ask(tab, cx);
}

/// The menu of feature modes.
pub fn features_menu(cx: &mut App) {
    crate::ui::features::toggle(cx);
}

/// The session menu: this shell's own, once Quickshell has stood its down.
pub fn session(cx: &mut App) {
    crate::ui::session::ask(crate::ui::Ask::Toggle, cx);
}

/// Opens the notification centre on `output`, or shuts it if that is where it
/// is open. The shell that is in use is the one that does it: asked down its
/// socket, the way a keybind asks, when this one is not it.
pub fn toggle_centre(cx: &App, output: String) {
    let desk = cx.global::<Desk>();
    if desk.serving {
        let server = desk.server.clone();
        return off_thread(cx, move || server.toggle_centre(&output));
    }
    off_thread(cx, || {
        if let Ok(mut socket) = UnixStream::connect(notifs::socket_path()) {
            let _ = socket.write_all(b"centre\n");
        }
    });
}

/// The notification centre, on the screen the person is looking at.
pub fn toggle_centre_here(ask: crate::ui::Ask, cx: &mut App) {
    // Shown and hidden are the same word to the server: it has one centre,
    // and asking again puts it away.
    let _ = ask;
    let output = hypr::focused_monitor().unwrap_or_default();
    toggle_centre(cx, output);
}

/// Every notification away, which is what the key for it does.
pub fn clear_notifications(cx: &App) {
    let desk = cx.global::<Desk>();
    if desk.serving {
        let server = desk.server.clone();
        return off_thread(cx, move || server.clear());
    }
    off_thread(cx, || {
        if let Ok(mut socket) = UnixStream::connect(notifs::socket_path()) {
            let _ = socket.write_all(b"clear\n");
        }
    });
}

/// The mark on the bar opens this shell's own launcher. A shell that has
/// none, because it was started to be looked at beside the one in use, knocks
/// on that one's door instead: a socket, so that a keybind costs a connect
/// and a few bytes rather than a program start.
pub fn toggle_launcher(cx: &mut App) {
    if crate::ui::launcher::toggle(cx) {
        return;
    }
    off_thread(cx, || {
        if let Ok(mut socket) = UnixStream::connect(cae_core::launcher::socket_path()) {
            let _ = socket.write_all(b"toggle\n");
        }
    });
}
