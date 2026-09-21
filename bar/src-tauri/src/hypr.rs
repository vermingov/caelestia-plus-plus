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
    /// Not drawn — the rows below carry their own flags — but watched.
    #[serde(skip)]
    pub view: View,
    pub workspaces: Vec<Workspace>,
    /// The named ones, which are reached by name rather than by walking the
    /// row: a scratchpad, a music workspace, a monitor. Only those with
    /// something on them are worth a pill.
    pub specials: Vec<Special>,
    pub active: Active,
    pub keyboard: Keyboard,
    /// The outputs whose desktop can be seen, by name. What moves on a
    /// desktop has no reason to while something is lying on it.
    pub desktops: Vec<String>,
}

/// What is in front of the person: the focused workspace, and the special
/// one pulled up over it, if any.
///
/// Read from the compositor's own answer rather than worked out from the rows:
/// those leave out what the bar does not draw — a named workspace, a special
/// one with nothing on it — and the view can go to either.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    workspace: i64,
    special: String,
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

/// Whether the window in front of the person is fullscreen.
///
/// Notifications ask this: a toast over a film or a game is an interruption
/// of a different order from one over a text editor, and the setting that
/// governs it is about exactly this case.
///
/// Hyprland reports 0 for a normal window, 1 for maximised and 2 for real
/// fullscreen. Maximised is still a window with a bar above it, so only 2 and
/// up count — which is the line the shell drew too.
pub fn fullscreen_focused() -> bool {
    let Some(reply) = request("j/activewindow") else { return false };
    let Ok(window) = serde_json::from_str::<serde_json::Value>(&reply) else { return false };
    window.get("fullscreen").and_then(serde_json::Value::as_i64).is_some_and(|mode| mode > 1)
}

/// The output with the focus, by name.
pub fn focused_monitor() -> Option<String> {
    let reply = request("j/monitors")?;
    serde_json::from_str::<Vec<serde_json::Value>>(&reply)
        .ok()?
        .into_iter()
        .find(|monitor| monitor.get("focused").and_then(serde_json::Value::as_bool) == Some(true))
        .and_then(|monitor| monitor.get("name").and_then(serde_json::Value::as_str).map(str::to_string))
}

/// Where a layer surface sits on one particular output. The plain
/// `layer_origin` takes the first it finds, which is only right for a surface
/// there is one of; the notification surfaces come one per output, all under
/// the same namespace.
pub fn layer_origin_on(output: &str, namespace: &str) -> Option<(i32, i32)> {
    let reply = request("j/layers")?;
    let outputs: serde_json::Value = serde_json::from_str(&reply).ok()?;
    let levels = outputs.get(output)?.get("levels")?.as_object()?;
    levels
        .values()
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .find(|layer| layer.get("namespace").and_then(serde_json::Value::as_str) == Some(namespace))
        .and_then(|layer| {
            Some((
                layer.get("x").and_then(serde_json::Value::as_i64)? as i32,
                layer.get("y").and_then(serde_json::Value::as_i64)? as i32,
            ))
        })
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
/// Every output, with its geometry in *logical* pixels.
///
/// Hyprland reports width and height in physical pixels and the scale
/// separately; GDK reports the logical size. Handing back the raw numbers
/// meant a fractionally scaled output never matched the GDK monitor beside
/// it — a 3440x1440 at 1.333 is 2580x1080 to GDK — so it fell through to a
/// made-up name and the bar on it never knew which screen it was.
pub fn monitors() -> Vec<(String, i32, i32, i32, i32)> {
    let Some(reply) = request("j/monitors") else { return Vec::new() };
    serde_json::from_str::<Vec<serde_json::Value>>(&reply)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|monitor| {
            let number = |key: &str| monitor.get(key).and_then(serde_json::Value::as_i64).unwrap_or(-1) as i32;
            let scale = monitor
                .get("scale")
                .and_then(serde_json::Value::as_f64)
                .filter(|scale| *scale > 0.0)
                .unwrap_or(1.0);
            // Rounded the way the compositor rounds it, so 3440/1.333 is the
            // 2580 GDK reports rather than 2580.2.
            let logical = |value: i32| (f64::from(value) / scale).round() as i32;
            Some((
                monitor.get("name").and_then(serde_json::Value::as_str)?.to_string(),
                number("x"),
                number("y"),
                logical(number("width")),
                logical(number("height")),
            ))
        })
        .collect()
}

/// Where a window is, for anything that has to line up with one: the
/// screenshot picker snaps its selection to whatever is under the pointer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Window {
    /// In the compositor's own coordinates, which span every screen.
    pub at: (i32, i32),
    pub size: (i32, i32),
    pub workspace: i64,
    pub floating: bool,
    pub pinned: bool,
    pub fullscreen: bool,
}

/// Every window that is on a screen, in the order they lie: whatever is
/// pinned first, then what is fullscreen, then what floats.
pub fn windows() -> Vec<Window> {
    let clients: Vec<serde_json::Value> =
        request("j/clients").and_then(|reply| serde_json::from_str(&reply).ok()).unwrap_or_default();
    let mut windows: Vec<Window> = clients
        .iter()
        .filter(|client| {
            let flag = |name: &str| client.get(name).and_then(serde_json::Value::as_bool).unwrap_or(false);
            flag("mapped") && !flag("hidden")
        })
        .filter_map(|client| {
            let pair = |name: &str| {
                let numbers = client.get(name)?.as_array()?;
                Some((numbers.first()?.as_i64()? as i32, numbers.get(1)?.as_i64()? as i32))
            };
            let flag = |name: &str| client.get(name).and_then(serde_json::Value::as_bool).unwrap_or(false);
            Some(Window {
                at: pair("at")?,
                size: pair("size")?,
                workspace: client.get("workspace")?.get("id")?.as_i64()?,
                floating: flag("floating"),
                pinned: flag("pinned"),
                fullscreen: client.get("fullscreen").and_then(serde_json::Value::as_i64).unwrap_or(0) > 0,
            })
        })
        .filter(|window| window.size.0 > 0 && window.size.1 > 0)
        .collect();
    windows.sort_by_key(|window| (!window.pinned, !window.fullscreen, !window.floating));
    windows
}

/// Which workspace each monitor is showing, by monitor name: the one that is
/// active, or the special one pulled up over it.
pub fn showing() -> Vec<(String, i64)> {
    let monitors: Vec<serde_json::Value> =
        request("j/monitors").and_then(|reply| serde_json::from_str(&reply).ok()).unwrap_or_default();
    monitors
        .iter()
        .filter_map(|monitor| {
            let name = monitor.get("name")?.as_str()?.to_string();
            let special = monitor.get("specialWorkspace").and_then(|w| w.get("id")).and_then(serde_json::Value::as_i64);
            let id = special.filter(|id| *id != 0).or_else(|| monitor.get("activeWorkspace")?.get("id")?.as_i64())?;
            Some((name, id))
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

/// Sets one of the compositor's options for as long as it is running. Nothing
/// is written to the config, so a reload puts back what it says.
pub fn keyword(name: &str, value: &str) {
    let _ = request(&format!("/keyword {name} {value}"));
}

/// Reads the config again, which undoes every keyword set above.
pub fn reload() {
    let _ = request("/reload");
}

/// A compositor option's value as a whole number, which is what the ones
/// that are switches are.
pub fn option(name: &str) -> Option<i64> {
    let reply = request(&format!("j/getoption {name}"))?;
    serde_json::from_str::<serde_json::Value>(&reply).ok()?.get("int")?.as_i64()
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

    let list = |question: &str| -> Vec<serde_json::Value> {
        request(question).and_then(|reply| serde_json::from_str(&reply).ok()).unwrap_or_default()
    };
    let monitors = list("j/monitors");

    // Whatever special workspace the focused monitor has pulled up, so the
    // row can show which one is open rather than just which exist.
    let open_special = monitors
        .iter()
        .find(|monitor| monitor.get("focused").and_then(serde_json::Value::as_bool).unwrap_or(false))
        .and_then(|monitor| {
            monitor
                .get("specialWorkspace")?
                .get("name")?
                .as_str()
                .map(|name| name.trim_start_matches("special:").to_string())
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

    State {
        view: View { workspace: focused_id, special: open_special },
        workspaces,
        specials,
        active,
        keyboard: read_keyboard(),
        desktops: seen_desktops(&monitors, &list("j/clients")),
    }
}

/// The outputs with nothing lying on their desktop: no window on the
/// workspace each is showing, or on the special one pulled up over it, that is
/// tiled or fullscreen. Floating windows leave most of a desktop in view,
/// which is the line the shell it came from drew too.
fn seen_desktops(monitors: &[serde_json::Value], clients: &[serde_json::Value]) -> Vec<String> {
    let id_of = |owner: &serde_json::Value, workspace: &str| owner.get(workspace).and_then(|w| w.get("id")).and_then(serde_json::Value::as_i64);
    let covers = |client: &&serde_json::Value| {
        let flag = |name: &str| client.get(name).and_then(serde_json::Value::as_bool).unwrap_or(false);
        let fullscreen = client.get("fullscreen").and_then(serde_json::Value::as_i64).unwrap_or(0) > 0;
        flag("mapped") && !flag("hidden") && (!flag("floating") || fullscreen)
    };
    let covered: Vec<i64> = clients.iter().filter(covers).filter_map(|client| id_of(client, "workspace")).collect();

    monitors
        .iter()
        .filter(|monitor| {
            // A special workspace that is not up has the id 0, which no
            // window is on.
            let showing = [id_of(monitor, "activeWorkspace"), id_of(monitor, "specialWorkspace")];
            !showing.into_iter().flatten().any(|id| covered.contains(&id))
        })
        .filter_map(|monitor| monitor.get("name").and_then(serde_json::Value::as_str).map(str::to_string))
        .collect()
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
        // Usually rides along with a change of active window, but not while a
        // layer surface holds the keyboard: then it is all there is.
        "activespecial",
        "openwindow",
        "closewindow",
        "movewindow",
        // Either takes a window off the desktop or lays it on.
        "changefloatingmode",
        "fullscreen",
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

    fn monitor(name: &str, workspace: i64, special: i64) -> serde_json::Value {
        serde_json::json!({ "name": name, "activeWorkspace": { "id": workspace }, "specialWorkspace": { "id": special } })
    }

    fn window(workspace: i64, floating: bool, fullscreen: i64) -> serde_json::Value {
        serde_json::json!({ "workspace": { "id": workspace }, "floating": floating, "fullscreen": fullscreen, "mapped": true, "hidden": false })
    }

    #[test]
    fn a_desktop_is_seen_until_something_is_tiled_or_fullscreen_on_it() {
        let monitors = [monitor("eDP-1", 1, 0), monitor("DP-2", 4, 0)];
        assert_eq!(seen_desktops(&monitors, &[]), ["eDP-1", "DP-2"]);
        // A floating window leaves the desktop in view; a tiled one does not.
        assert_eq!(seen_desktops(&monitors, &[window(1, true, 0), window(4, false, 0)]), ["eDP-1"]);
        // A floating window gone fullscreen covers it like any other.
        assert_eq!(seen_desktops(&monitors, &[window(1, true, 2)]), ["DP-2"]);
        // What is on a workspace nobody is looking at covers nothing.
        assert_eq!(seen_desktops(&monitors, &[window(7, false, 0)]), ["eDP-1", "DP-2"]);
    }

    #[test]
    fn a_special_workspace_pulled_up_lies_on_the_desktop_too() {
        let scratchpad = window(-98, false, 0);
        assert_eq!(seen_desktops(&[monitor("eDP-1", 1, -98)], std::slice::from_ref(&scratchpad)), [] as [&str; 0]);
        assert_eq!(seen_desktops(&[monitor("eDP-1", 1, 0)], &[scratchpad]), ["eDP-1"]);
    }

    #[test]
    fn a_window_that_is_not_on_screen_covers_nothing() {
        let mut unmapped = window(1, false, 0);
        unmapped["mapped"] = false.into();
        let mut hidden = window(1, false, 0);
        hidden["hidden"] = true.into();
        assert_eq!(seen_desktops(&[monitor("eDP-1", 1, 0)], &[unmapped, hidden]), ["eDP-1"]);
    }
}
