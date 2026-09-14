//! The Caelestia++ application launcher.
//!
//! A resident process: the window is built once at startup and then shown and
//! hidden, so opening it is a compositor commit rather than a program start.
//! A second invocation with `--toggle` does not start anything — it connects
//! to the running one over a socket in the runtime directory and asks it to
//! show itself.
//!
//! The window is a layer surface (gtk-layer-shell), not a floating window: a
//! launcher needs the keyboard to itself, has to sit above everything, and
//! must not appear in the window list or be tiled by the compositor. Its
//! namespace is `caelestia-launcher`, which is what the Hyprland layer rule
//! matches to blur behind it.

mod apps;
mod icons;
mod search;
mod usage;

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Launcher {
    apps: Vec<apps::App>,
    usage: usage::Usage,
    icons: icons::Icons,
}

#[derive(Serialize)]
pub struct Entry {
    id: String,
    name: String,
    comment: String,
    /// Absolute path to the icon file, or empty when the theme has none.
    icon: String,
}

#[tauri::command]
fn search(query: String, state: State<'_, Mutex<Launcher>>) -> Vec<Entry> {
    let launcher = state.lock().expect("the launcher state is not poisoned");
    search::rank(&launcher.apps, &query, &launcher.usage)
        .into_iter()
        // The list is virtualised in the UI, but sending a thousand entries
        // per keystroke over the IPC bridge is what makes a launcher feel
        // slow, so the tail nobody scrolls to is never serialised.
        .take(64)
        .map(|app| Entry {
            id: app.id.clone(),
            name: app.name.clone(),
            comment: app.comment.clone(),
            icon: launcher.icons.resolve(&app.icon).unwrap_or_default(),
        })
        .collect()
}

#[tauri::command]
fn launch(id: String, app: AppHandle, state: State<'_, Mutex<Launcher>>) -> Result<(), String> {
    let mut launcher = state.lock().map_err(|_| "launcher state is poisoned")?;
    let entry = launcher
        .apps
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("no application called {id}"))?;

    let command = if entry.terminal {
        // A terminal entry needs one; the shell's own default is the only
        // one the rest of the desktop agrees on.
        format!("{} -e {}", terminal(), entry.exec)
    } else {
        entry.exec.clone()
    };

    // Detached and out of this process's tree: the launcher hides itself
    // immediately, and nothing it started should die with it or inherit its
    // layer-surface environment.
    std::process::Command::new("sh")
        .args(["-c", &format!("setsid -f {command} >/dev/null 2>&1")])
        .spawn()
        .map_err(|e| format!("cannot start {command}: {e}"))?;

    launcher.usage.record(&id);
    hide(&app);
    Ok(())
}

fn terminal() -> String {
    std::env::var("TERMINAL").unwrap_or_else(|_| "foot".to_string())
}

#[tauri::command]
fn dismiss(app: AppHandle) {
    hide(&app);
}

fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("launcher") {
        let _ = window.hide();
    }
}

fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window("launcher") else { return };
    // The frontend clears its query and refocuses on this, so the launcher
    // never reopens showing the last thing that was typed.
    let _ = app.emit("launcher-opened", ());
    let _ = window.show();
    let _ = window.set_focus();
}

fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window("launcher") else { return };
    if window.is_visible().unwrap_or(false) {
        hide(app);
    } else {
        show(app);
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![search, launch, dismiss])
        .setup(|app| {
            let window = app.get_webview_window("launcher").expect("the launcher window exists");
            platform::place(&window);

            app.manage(Mutex::new(Launcher {
                apps: apps::load(),
                usage: usage::Usage::load(),
                icons: icons::Icons::new(),
            }));

            control::listen(app.handle().clone());

            // Clicking away dismisses it. A launcher that stays up after you
            // have looked somewhere else is a window, not a launcher.
            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    hide(&handle);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the launcher could not start");
}

/// The socket a second invocation talks to, so `--toggle` is a connect and a
/// byte rather than a program start.
mod control {
    use std::io::Read;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    use tauri::AppHandle;

    pub fn socket_path() -> PathBuf {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        runtime.join("caelestia-launcher.sock")
    }

    /// Asks a running launcher to toggle. False when there is nothing
    /// listening, which is how the caller knows it has to start one.
    pub fn send(word: &str) -> bool {
        use std::io::Write;
        let Ok(mut stream) = UnixStream::connect(socket_path()) else { return false };
        stream.write_all(word.as_bytes()).is_ok()
    }

    pub fn listen(app: AppHandle) {
        let path = socket_path();
        // A socket left behind by a crashed launcher would keep every later
        // one from binding, and nothing else owns this path.
        let _ = std::fs::remove_file(&path);
        let Ok(listener) = UnixListener::bind(&path) else {
            eprintln!("caelestia-launcher: cannot listen on {}", path.display());
            return;
        };

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut word = String::new();
                if stream.read_to_string(&mut word).is_err() {
                    continue;
                }
                let app = app.clone();
                // Window calls have to happen on the main thread.
                let _ = app.clone().run_on_main_thread(move || match word.trim() {
                    "show" => super::show(&app),
                    "hide" => super::hide(&app),
                    _ => super::toggle(&app),
                });
            }
        });
    }
}

pub use control::send as send_control;

/// Everything that needs the window underneath Tauri.
mod platform {
    use tauri::WebviewWindow;

    /// A launcher belongs on the overlay layer with the keyboard to itself.
    /// With gtk-layer-shell that is exactly what it gets; without, it is an
    /// always-on-top window placed where the layer surface would have been,
    /// which every compositor can do.
    #[cfg(feature = "layer-shell")]
    pub fn place(window: &WebviewWindow) {
        use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

        let Ok(gtk_window) = window.gtk_window() else {
            eprintln!("caelestia-launcher: no GTK window; falling back to a plain one");
            return place_plain(window);
        };

        // Must happen before the window is first shown: a realised window
        // cannot become a layer surface.
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        // Exclusive, or what is typed goes to whatever had focus before.
        gtk_window.set_keyboard_mode(KeyboardMode::Exclusive);
        // What the compositor's blur rule matches on.
        gtk_window.set_namespace("caelestia-launcher");

        // Anchored to the top and centred, a fifth of the way down: where the
        // eye already is, and clear of what is being searched over.
        gtk_window.set_anchor(Edge::Top, true);
        gtk_window.set_margin(Edge::Top, 220);
        for edge in [Edge::Left, Edge::Right, Edge::Bottom] {
            gtk_window.set_anchor(edge, false);
        }
    }

    #[cfg(not(feature = "layer-shell"))]
    pub fn place(window: &WebviewWindow) {
        place_plain(window);
    }

    fn place_plain(window: &WebviewWindow) {
        let _ = window.set_always_on_top(true);
        let _ = window.set_skip_taskbar(true);
        if let Err(e) = window.center() {
            eprintln!("caelestia-launcher: cannot centre the window: {e}");
        }
    }
}
