//! What the pages can ask of the notification server.
//!
//! All `(async)`: none of them touch a window directly, and several end in a
//! D-Bus signal or a compositor round trip — see the note above the commands
//! in `lib.rs` for what a synchronous one costs.

use std::sync::Mutex;

use tauri::{State, WebviewWindow};

use super::{reason, window, Feed, Notifs, Summary};
use crate::{Outputs, Rect};

/// The output a window is drawn on, from the table the bars filled in.
fn output_of(window: &WebviewWindow, outputs: &State<'_, Mutex<Outputs>>) -> String {
    outputs.lock().ok().and_then(|outputs| outputs.0.get(window.label()).cloned()).unwrap_or_default()
}

/// The list as it stands, for a window that has only just loaded and has not
/// been pushed anything yet.
#[tauri::command(async)]
pub fn notifs(server: State<'_, Notifs>) -> Feed {
    server.feed()
}

/// The same for a bar, which only draws a bell.
#[tauri::command(async)]
pub fn notifs_summary(server: State<'_, Notifs>) -> Summary {
    Summary::of(&server.feed())
}

/// How the person has asked for notifications to behave, from shell.json.
#[tauri::command(async)]
pub fn notifs_config() -> crate::logo::NotifsConfig {
    crate::logo::notifs_config()
}

/// Takes one toast off the screen. The notification stays in the list.
#[tauri::command(async)]
pub fn notif_dismiss(id: u32, server: State<'_, Notifs>) {
    server.dismiss_popup(id);
}

/// Throws one away and tells whoever sent it.
#[tauri::command(async)]
pub fn notif_close(id: u32, server: State<'_, Notifs>) {
    server.close(id, reason::DISMISSED);
}

/// Throws away everything one application sent.
#[tauri::command(async)]
pub fn notif_close_app(app_name: String, server: State<'_, Notifs>) {
    server.close_app(&app_name);
}

#[tauri::command(async)]
pub fn notif_clear(server: State<'_, Notifs>) {
    server.clear();
}

/// Presses one of the sender's own buttons.
#[tauri::command(async)]
pub fn notif_action(id: u32, action: String, server: State<'_, Notifs>) {
    server.invoke(id, &action);
}

#[tauri::command(async)]
pub fn notif_dnd(on: bool, server: State<'_, Notifs>) {
    server.set_dnd(on);
}

/// The pointer has come to rest on a toast, or left it again.
#[tauri::command(async)]
pub fn notif_hold(id: u32, held: bool, server: State<'_, Notifs>) {
    if held {
        server.hold(id);
    } else {
        server.release(id);
    }
}

/// Opens, shuts or flips the centre on the output the asking window is on.
///
/// `hover` is the pointer reaching the corner of the screen rather than
/// anybody pressing anything, and that is refused over a fullscreen window:
/// the corner of a game is part of the game.
#[tauri::command(async)]
pub fn notif_centre(
    open: Option<bool>,
    hover: Option<bool>,
    window: WebviewWindow,
    server: State<'_, Notifs>,
    outputs: State<'_, Mutex<Outputs>>,
) {
    if hover == Some(true) && crate::hypr::fullscreen_focused() {
        return;
    }
    let output = output_of(&window, &outputs);
    match open {
        Some(true) => server.set_centre(&output),
        Some(false) => server.set_centre(""),
        None => server.toggle_centre(&output),
    }
}

/// Which parts of the notification surface accept the pointer: the boxes the
/// page has drawn something in, and nothing else.
#[tauri::command(async)]
pub fn notifs_reach(boxes: Vec<Rect>, window: WebviewWindow) {
    window::set_reach(&window, boxes);
}

/// Where the pointer is on the asking surface, from the compositor.
#[tauri::command(async)]
pub fn notifs_pointer(window: WebviewWindow, outputs: State<'_, Mutex<Outputs>>) -> Option<(i32, i32)> {
    window::pointer(&window, &output_of(&window, &outputs))
}

/// Opens a link from a notification's body in the browser.
///
/// The body is written by whoever sent the notification, which includes any
/// website allowed to send them, so the address is never handed to a shell
/// and only ever to `xdg-open`, as one argument, if it is a kind of link a
/// notification has any business containing.
#[tauri::command(async)]
pub fn notif_open_link(url: String) {
    let allowed = ["https://", "http://", "mailto:"];
    if !allowed.iter().any(|scheme| url.starts_with(scheme)) {
        return;
    }
    // Waited for, on this worker thread: a child that is spawned and never
    // waited on stays in the process table as a zombie for as long as the
    // bar runs, one per link ever opened.
    let _ = std::process::Command::new("xdg-open").arg(url).status();
}

/// Puts a notification's text on the clipboard.
///
/// Through `wl-copy` rather than the page's own clipboard API, which WebKit
/// only grants to a page it considers to have been given a gesture — and a
/// click on a layer surface that never takes the keyboard does not count.
/// The text goes down stdin, so nothing a sender wrote is ever an argument.
#[tauri::command(async)]
pub fn notif_copy(text: String) {
    use std::io::Write;

    let Ok(mut child) = std::process::Command::new("wl-copy").stdin(std::process::Stdio::piped()).spawn() else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
}

/// Somewhere for the notification page to say what went wrong.
///
/// Always on, unlike the bar's `diag`: this surface is transparent and nearly
/// always empty, so a page that failed to start looks exactly like one with
/// nothing to show, and the first anybody hears of it is a notification that
/// never appeared. Only error paths and the one line at startup call it.
#[tauri::command(async)]
pub fn notifs_log(message: String, window: WebviewWindow) {
    eprintln!("caelestia-bar[{}]: {message}", window.label());
}
