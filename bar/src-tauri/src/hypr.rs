//! Hyprland, as the bar needs it: which workspaces exist, which one is
//! focused, and what is in front on it.
//!
//! Two sockets, because Hyprland has two. `.socket.sock` answers questions
//! and takes dispatches; `.socket2.sock` is a line-per-event stream that
//! never answers anything. The stream is what makes the bar react at the
//! moment something happens rather than a tick later, and the request socket
//! is what fills in the detail the event line leaves out.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::OnceLock;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    /// Windows on it. An empty workspace that is not focused is not drawn.
    pub windows: i64,
    pub focused: bool,
    pub monitor: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Active {
    pub title: String,
    pub class: String,
}

/// The keyboard, as the status row shows it: the layout in use and whichever
/// of the two locks are on.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Keyboard {
    pub layout: String,
    pub caps_lock: bool,
    pub num_lock: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub workspaces: Vec<Workspace>,
    /// The named ones, which are reached by name rather than by walking the
    /// row: a scratchpad, a music workspace, a monitor. Only those with
    /// something on them are worth a pill.
    pub specials: Vec<Special>,
    pub active: Active,
    pub keyboard: Keyboard,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Special {
    /// Without the "special:" prefix, which is how a person names it and how
    /// the dispatcher wants it.
    pub name: String,
    pub windows: i64,
    /// Whether it is the one currently pulled up on the focused monitor.
    pub open: bool,
}

fn socket_dir() -> Option<PathBuf> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    Some(PathBuf::from(runtime).join("hypr").join(signature))
}

/// Asks the request socket something and returns the raw reply.
fn request(message: &str) -> Option<String> {
    let mut socket = UnixStream::connect(socket_dir()?.join(".socket.sock")).ok()?;
    socket.write_all(message.as_bytes()).ok()?;
    let mut reply = String::new();
    socket.read_to_string(&mut reply).ok()?;
    Some(reply)
}

/// Where the pointer is, in screen coordinates.
///
/// Asked of the compositor rather than of the webview: a pointer that leaves
/// the surface's input region does not always produce an event in the page,
/// and the hover state the engine keeps is stale for exactly the same reason.
/// This is the one source that is never wrong.
pub fn cursor() -> Option<(i32, i32)> {
    let reply = request("cursorpos")?;
    let (x, y) = reply.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Where a layer surface of ours sits on the screen, so a position inside the
/// page can be compared with one outside it.
pub fn layer_origin(namespace: &str) -> Option<(i32, i32)> {
    let reply = request("j/layers")?;
    let outputs: serde_json::Value = serde_json::from_str(&reply).ok()?;
    for (_, output) in outputs.as_object()? {
        for (_, layers) in output.get("levels")?.as_object()? {
            for layer in layers.as_array()? {
                if layer.get("namespace").and_then(serde_json::Value::as_str) == Some(namespace) {
                    return Some((
                        layer.get("x").and_then(serde_json::Value::as_i64)? as i32,
                        layer.get("y").and_then(serde_json::Value::as_i64)? as i32,
                    ));
                }
            }
        }
    }
    None
}

/// Every output Hyprland knows about, with its place on the desktop.
///
/// The bar builds one surface per output and has to know which is which; GDK
/// numbers its monitors and Hyprland names them, and the geometry is the only
/// thing both agree on.
pub fn monitors() -> Vec<(String, i32, i32, i32, i32)> {
    let Some(reply) = request("j/monitors") else { return Vec::new() };
    serde_json::from_str::<Vec<serde_json::Value>>(&reply)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|monitor| {
            let number = |key: &str| monitor.get(key).and_then(serde_json::Value::as_i64).unwrap_or(-1) as i32;
            Some((
                monitor.get("name").and_then(serde_json::Value::as_str)?.to_string(),
                number("x"),
                number("y"),
                number("width"),
                number("height"),
            ))
        })
        .collect()
}

/// Pulls a special workspace up, or puts it away if it is already up.
pub fn toggle_special(name: &str) {
    dispatch(&format!("togglespecialworkspace {name}"));
}

pub fn dispatch(command: &str) {
    let _ = request(&format!("/dispatch {command}"));
}

/// The whole picture, read fresh. Three requests rather than one: Hyprland has
/// no combined form, and each is a local socket round trip well under a
/// millisecond.
pub fn read_state() -> State {
    let workspaces = request("j/workspaces").unwrap_or_default();
    let active_workspace = request("j/activeworkspace").unwrap_or_default();
    let active_window = request("j/activewindow").unwrap_or_default();

    let focused_id = serde_json::from_str::<serde_json::Value>(&active_workspace)
        .ok()
        .and_then(|v| v.get("id").and_then(serde_json::Value::as_i64))
        .unwrap_or(1);

    let parsed = serde_json::from_str::<Vec<serde_json::Value>>(&workspaces).unwrap_or_default();

    // Whatever special workspace the focused monitor has pulled up, so the
    // row can show which one is open rather than just which exist.
    let open_special = request("j/monitors")
        .and_then(|monitors| serde_json::from_str::<Vec<serde_json::Value>>(&monitors).ok())
        .and_then(|monitors| {
            monitors
                .into_iter()
                .find(|monitor| {
                    monitor.get("focused").and_then(serde_json::Value::as_bool).unwrap_or(false)
                })
                .and_then(|monitor| {
                    monitor
                        .get("specialWorkspace")?
                        .get("name")?
                        .as_str()
                        .map(|name| name.trim_start_matches("special:").to_string())
                })
        })
        .unwrap_or_default();

    let specials: Vec<Special> = parsed
        .iter()
        .filter(|w| w.get("id").and_then(serde_json::Value::as_i64).unwrap_or(0) < 0)
        .map(|w| {
            let name = w
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim_start_matches("special:")
                .to_string();
            Special {
                windows: w.get("windows").and_then(serde_json::Value::as_i64).unwrap_or(0),
                open: !name.is_empty() && name == open_special,
                name,
            }
        })
        // An empty special workspace is one Hyprland has not cleaned up yet,
        // not one there is anything to go to.
        .filter(|special| special.windows > 0)
        .collect();

    let mut workspaces: Vec<Workspace> = parsed
        .into_iter()
        .into_iter()
        .map(|w| {
            let id = w.get("id").and_then(serde_json::Value::as_i64).unwrap_or(0);
            Workspace {
                id,
                name: w
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                windows: w.get("windows").and_then(serde_json::Value::as_i64).unwrap_or(0),
                focused: id == focused_id,
                monitor: w
                    .get("monitor")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }
        })
        // Special workspaces are reached by name, not by walking the row, and
        // drawing them among the numbers makes the row jump about.
        .filter(|w| w.id > 0)
        .collect();
    workspaces.sort_by_key(|w| w.id);

    let active = serde_json::from_str::<serde_json::Value>(&active_window)
        .ok()
        .map(|w| Active {
            title: w.get("title").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
            class: w.get("class").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
        })
        .unwrap_or_default();

    State { workspaces, specials, active, keyboard: read_keyboard() }
}

/// The main keyboard's layout and locks.
///
/// Hyprland reports every keyboard it has, most of which are not one — the
/// one flagged `main` is the one whose layout is actually in effect.
fn read_keyboard() -> Keyboard {
    let devices = request("j/devices").unwrap_or_default();
    let Ok(devices) = serde_json::from_str::<serde_json::Value>(&devices) else {
        return Keyboard::default();
    };
    let keyboards = devices.get("keyboards").and_then(serde_json::Value::as_array);
    let Some(keyboard) = keyboards.and_then(|list| {
        list.iter()
            .find(|k| k.get("main").and_then(serde_json::Value::as_bool).unwrap_or(false))
            .or_else(|| list.first())
    }) else {
        return Keyboard::default();
    };

    let flag = |name: &str| keyboard.get(name).and_then(serde_json::Value::as_bool).unwrap_or(false);
    let keymap = keyboard.get("active_keymap").and_then(serde_json::Value::as_str).unwrap_or_default();
    Keyboard {
        layout: short_layout(keymap),
        caps_lock: flag("capsLock"),
        num_lock: flag("numLock"),
    }
}

/// Turns what Hyprland calls the layout — "Danish", "English (US)" — into the
/// two-letter code the bar has room for.
///
/// X11 ships the mapping, so it is read rather than guessed at: a hard-coded
/// table would be wrong for exactly the layouts nobody testing it uses.
fn short_layout(keymap: &str) -> String {
    if keymap.is_empty() {
        return String::new();
    }
    layout_codes()
        .get(keymap)
        .cloned()
        .unwrap_or_else(|| keymap.chars().take(2).collect::<String>())
        .to_uppercase()
}

/// Description to code, from the xkb rules list. Built once.
fn layout_codes() -> &'static HashMap<String, String> {
    static CODES: OnceLock<HashMap<String, String>> = OnceLock::new();
    CODES.get_or_init(|| {
        let mut codes = HashMap::new();
        let Ok(list) = std::fs::read_to_string("/usr/share/X11/xkb/rules/base.lst") else {
            return codes;
        };

        // The file is sections headed by `! layout`, `! variant` and so on.
        // Only the first two say anything about what a keymap is called.
        let mut section = "";
        for line in list.lines() {
            if let Some(name) = line.strip_prefix("! ") {
                section = name.trim();
                continue;
            }
            let Some((code, description)) = line.trim().split_once(char::is_whitespace) else {
                continue;
            };
            let description = description.trim();
            match section {
                // "  dk    Danish"
                "layout" => {
                    codes.insert(description.to_string(), code.to_string());
                }
                // "  chr   us: Cherokee" — the variant is shown as its base
                // layout, which is what the key actually types as.
                "variant" => {
                    if let Some((base, variant)) = description.split_once(':') {
                        codes.entry(variant.trim().to_string()).or_insert_with(|| base.trim().to_string());
                    }
                }
                _ => {}
            }
        }
        codes
    })
}

/// Calls `on_change` whenever Hyprland says something that could have moved
/// the bar. Blocks; meant for its own thread.
///
/// The event name is not trusted to describe the change — it is only used to
/// decide whether to look. Hyprland's event vocabulary shifts between
/// versions, and a bar that parses each line is a bar that breaks on upgrade.
pub fn watch(mut on_change: impl FnMut(State)) {
    const INTERESTING: &[&str] = &[
        "workspace",
        "focusedmon",
        "activewindow",
        "openwindow",
        "closewindow",
        "movewindow",
        "createworkspace",
        "destroyworkspace",
        "urgent",
        "windowtitle",
        // An output coming or going means a bar has to be built or closed.
        "monitoradded",
        "monitorremoved",
        // The locks and the layout live in the same payload as the rest.
        "activelayout",
    ];

    // Replaced by a real reading as soon as the socket is up.
    let mut last;
    loop {
        let Some(path) = socket_dir().map(|d| d.join(".socket2.sock")) else {
            eprintln!("caelestia-bar: not running under Hyprland");
            return;
        };
        let Ok(stream) = UnixStream::connect(&path) else {
            // Hyprland restarting, or not up yet. Neither is an error worth
            // exiting over: the bar outlives a compositor reload.
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        };

        // The first read is unconditional: whatever happened while the socket
        // was down is already in the past.
        let fresh = read_state();
        last = fresh.clone();
        on_change(fresh);

        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let name = line.split(">>").next().unwrap_or_default();
            if !INTERESTING.iter().any(|e| name.starts_with(e)) {
                continue;
            }
            let fresh = read_state();
            // Hyprland is chatty — a single window move is several lines. Only
            // a change that the bar would actually draw differently is worth
            // waking the webview for.
            if fresh != last {
                last = fresh.clone();
                on_change(fresh);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_layout_becomes_its_code() {
        // Read from the machine's own xkb list, so this only asserts what the
        // file actually says.
        if layout_codes().is_empty() {
            return; // no xkb data installed; nothing to assert against
        }
        assert_eq!(short_layout("Danish"), "DK");
        assert_eq!(short_layout("English (US)"), "US");
    }

    #[test]
    fn an_unknown_layout_falls_back_to_its_first_letters() {
        assert_eq!(short_layout("Klingon"), "KL");
    }

    #[test]
    fn no_keyboard_is_no_label() {
        assert_eq!(short_layout(""), "");
    }
}
