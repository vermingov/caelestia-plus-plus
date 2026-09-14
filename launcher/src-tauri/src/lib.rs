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

mod actions;
mod apps;
mod calc;
mod config;
mod icons;
mod modes;
mod schemes;
mod search;
mod usage;
mod variants;
mod wallpapers;

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Launcher {
    apps: Vec<apps::App>,
    usage: usage::Usage,
    icons: icons::Icons,
    config: config::Config,
    schemes: Vec<schemes::Scheme>,
    wallpapers: Vec<wallpapers::Wallpaper>,
    /// What is set now, so the lists can mark it.
    current_scheme: String,
    current_variant: String,
    current_wallpaper: String,
}

impl Launcher {
    fn new() -> Launcher {
        let config = config::Config::load();
        let (current_scheme, current_variant) = schemes::current();
        Launcher {
            apps: apps::load(),
            usage: usage::Usage::load(),
            icons: icons::Icons::new(),
            schemes: schemes::load(),
            wallpapers: wallpapers::load(&config.wallpaper_dir),
            current_wallpaper: wallpapers::current().unwrap_or_default(),
            current_scheme,
            current_variant,
            config,
        }
    }

    /// Re-read everything that can change while the launcher sits hidden: the
    /// config, the app list, the wallpapers, the scheme in use.
    fn refresh(&mut self) {
        self.config = config::Config::load();
        self.apps = apps::load();
        self.usage = usage::Usage::load();
        self.schemes = schemes::load();
        self.wallpapers = wallpapers::load(&self.config.wallpaper_dir);
        self.current_wallpaper = wallpapers::current().unwrap_or_default();
        let (scheme, variant) = schemes::current();
        self.current_scheme = scheme;
        self.current_variant = variant;
    }
}

/// One row, whatever mode produced it. The front end draws by `kind` — an
/// app has an icon file, a scheme has swatches, a glyph-based row has a
/// Material icon name — so one list component covers every mode.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// What activating this row acts on.
    pub id: String,
    pub name: String,
    pub comment: String,
    /// Absolute path to an icon file, for app rows.
    pub icon: String,
    /// A Material Symbols name, for rows without a file icon.
    pub glyph: String,
    /// `rrggbb` strip, for scheme rows.
    pub swatches: Vec<String>,
    /// The image to show, for wallpaper rows.
    pub preview: String,
    /// The label at the right end: the kind of thing this row is.
    pub trailing: String,
    /// Marked in the list: a favourite app, the scheme already in use.
    pub marked: bool,
}

impl Default for Entry {
    fn default() -> Entry {
        Entry {
            id: String::new(),
            name: String::new(),
            comment: String::new(),
            icon: String::new(),
            glyph: String::new(),
            swatches: Vec::new(),
            preview: String::new(),
            trailing: String::new(),
            marked: false,
        }
    }
}

/// Everything the front end needs to draw one keystroke's worth of launcher.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    /// The mode's own name, so the UI can lay wallpapers out as a grid.
    pub mode: String,
    /// How many rows to show before the list scrolls, from the shell's config.
    pub max_shown: usize,
    /// The heading over the list.
    pub label: String,
    /// What Enter will do.
    pub action: String,
    pub entries: Vec<Entry>,
}

/// Ranks whatever the current mode lists against the query.
///
/// Every mode goes through here on every keystroke: ranking a thousand apps
/// takes a few hundred microseconds, and a launcher that lags the keyboard is
/// the one thing it may not do.
#[tauri::command]
fn search(query: String, state: State<'_, Mutex<Launcher>>) -> Results {
    let launcher = state.lock().expect("the launcher state is not poisoned");
    let (mode, argument) = modes::parse(&query, &launcher.config);

    let entries = match mode {
        modes::Mode::Apps => app_entries(&launcher, argument),
        modes::Mode::Actions => action_entries(&launcher, argument),
        modes::Mode::Calc => calc_entries(argument),
        modes::Mode::Scheme => scheme_entries(&launcher, argument),
        modes::Mode::Variant => variant_entries(&launcher, argument),
        modes::Mode::Wallpaper => wallpaper_entries(&launcher, argument),
    };

    Results {
        max_shown: launcher.config.launcher.max_shown.clamp(3, 20),
        mode: mode.name().to_string(),
        label: mode.label().to_string(),
        action: mode.action().to_string(),
        entries,
    }
}

fn app_entries(launcher: &Launcher, query: &str) -> Vec<Entry> {
    let hidden = &launcher.config.launcher.hidden_apps;
    let favourites = &launcher.config.launcher.favourite_apps;

    search::rank(&launcher.apps, query, &launcher.usage)
        .into_iter()
        .filter(|app| !config::matches_any(hidden, &app.id))
        // The list is capped before it is serialised: sending a thousand
        // entries per keystroke over the IPC bridge is what makes a launcher
        // feel slow, and nobody scrolls that far.
        .take(64)
        .map(|app| Entry {
            id: app.id.clone(),
            name: app.name.clone(),
            comment: app.comment.clone(),
            icon: launcher.icons.resolve(&app.icon).unwrap_or_default(),
            trailing: "Application".to_string(),
            marked: config::matches_any(favourites, &app.id),
            ..Entry::default()
        })
        .collect()
}

fn action_entries(launcher: &Launcher, query: &str) -> Vec<Entry> {
    let needle = query.trim().to_lowercase();
    launcher
        .config
        .usable_actions()
        .into_iter()
        .filter(|action| {
            needle.is_empty()
                || action.name.to_lowercase().contains(&needle)
                || action.description.to_lowercase().contains(&needle)
        })
        .map(|action| Entry {
            id: action.name.clone(),
            name: action.name.clone(),
            comment: action.description.clone(),
            glyph: action.icon.clone(),
            trailing: if action.dangerous { "Dangerous".into() } else { "Command".into() },
            ..Entry::default()
        })
        .collect()
}

/// One row, which is the answer. Enter copies it; the row also offers opening
/// the expression in a real calculator.
fn calc_entries(expression: &str) -> Vec<Entry> {
    let (name, id) = match calc::evaluate(expression) {
        Some(answer) => answer,
        None => (
            if expression.trim().is_empty() {
                "Type an expression to calculate".to_string()
            } else {
                "Calculating…".to_string()
            },
            String::new(),
        ),
    };
    vec![Entry {
        id,
        marked: calc::is_error(&name),
        name,
        glyph: "function".to_string(),
        comment: expression.trim().to_string(),
        trailing: "Calculator".to_string(),
        ..Entry::default()
    }]
}

fn scheme_entries(launcher: &Launcher, query: &str) -> Vec<Entry> {
    let needle = query.trim().to_lowercase();
    launcher
        .schemes
        .iter()
        .filter(|scheme| needle.is_empty() || scheme.haystack.contains(&needle))
        .map(|scheme| {
            let full = format!("{} {}", scheme.name, scheme.flavour);
            Entry {
                id: format!("{}/{}", scheme.name, scheme.flavour),
                name: capitalise(&scheme.name),
                comment: capitalise(&scheme.flavour),
                swatches: scheme.swatches.clone(),
                trailing: "Scheme".to_string(),
                marked: full == launcher.current_scheme,
                ..Entry::default()
            }
        })
        .collect()
}

fn variant_entries(launcher: &Launcher, query: &str) -> Vec<Entry> {
    let needle = query.trim().to_lowercase();
    variants::ALL
        .iter()
        .filter(|variant| {
            needle.is_empty()
                || variant.name.to_lowercase().contains(&needle)
                || variant.id.contains(&needle)
        })
        .map(|variant| Entry {
            id: variant.id.to_string(),
            name: variant.name.to_string(),
            comment: variant.description.to_string(),
            glyph: variant.icon.to_string(),
            trailing: "Variant".to_string(),
            marked: variant.id == launcher.current_variant,
            ..Entry::default()
        })
        .collect()
}

fn wallpaper_entries(launcher: &Launcher, query: &str) -> Vec<Entry> {
    let needle = query.trim().to_lowercase();
    launcher
        .wallpapers
        .iter()
        .filter(|wallpaper| needle.is_empty() || wallpaper.haystack.contains(&needle))
        .take(64)
        .map(|wallpaper| Entry {
            id: wallpaper.path.clone(),
            name: wallpaper.name.clone(),
            comment: wallpaper.category.clone(),
            preview: wallpaper.preview.clone(),
            trailing: "Wallpaper".to_string(),
            marked: wallpaper.path == launcher.current_wallpaper,
            ..Entry::default()
        })
        .collect()
}

/// Scheme and flavour names are stored lowercase; the list reads better with
/// them capitalised, and nothing downstream sees this form.
fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What Enter does, which depends on the mode the query is in.
///
/// Returns the text the search box should now hold: empty means the launcher
/// is done and closing, anything else means a command asked to lead the user
/// somewhere (an `autocomplete` action, or the calculator handing off).
#[tauri::command]
fn activate(query: String, id: String, app: AppHandle, state: State<'_, Mutex<Launcher>>) -> String {
    let mut launcher = state.lock().expect("the launcher state is not poisoned");
    let (mode, _) = modes::parse(&query, &launcher.config);

    match mode {
        modes::Mode::Apps => {
            if let Some(entry) = launcher.apps.iter().find(|a| a.id == id) {
                let command = if entry.terminal {
                    format!("{} -e {}", launcher.config.terminal.join(" "), entry.exec)
                } else {
                    entry.exec.clone()
                };
                spawn_detached(&command);
                launcher.usage.record(&id);
            }
        }
        modes::Mode::Actions => {
            let config = launcher.config.clone();
            if let Some(action) = config.usable_actions().into_iter().find(|a| a.name == id) {
                if let actions::Outcome::Autocomplete(text) = actions::run(action, &config) {
                    // Stays open: the point of the action was to get here.
                    return text;
                }
            }
        }
        modes::Mode::Calc => {
            if id.is_empty() {
                return query; // nothing to copy yet
            }
            copy_to_clipboard(&id);
        }
        modes::Mode::Scheme => {
            if let Some((name, flavour)) = id.split_once('/') {
                schemes::apply(name, flavour);
            }
        }
        modes::Mode::Variant => schemes::apply_variant(&id),
        modes::Mode::Wallpaper => wallpapers::set(&id),
    }

    hide(&app);
    String::new()
}

/// Opens the current calculation in a real calculator, which is the one
/// thing a one-line answer cannot do.
#[tauri::command]
fn open_in_calculator(expression: String, app: AppHandle, state: State<'_, Mutex<Launcher>>) {
    let expression = expression.trim();
    if expression.is_empty() {
        return;
    }
    let terminal = {
        let launcher = state.lock().expect("the launcher state is not poisoned");
        launcher.config.terminal.join(" ")
    };
    let quoted = expression.replace('\'', r"'\''");
    spawn_detached(&format!("{terminal} fish -C \"exec qalc -i '{quoted}'\""));
    hide(&app);
}

fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    use std::process::Stdio;

    let Ok(mut child) = std::process::Command::new("wl-copy").stdin(Stdio::piped()).spawn() else {
        return;
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
}

/// Out of this process's tree: the launcher hides immediately, and nothing it
/// starts should die with it or inherit its layer-surface environment.
fn spawn_detached(command: &str) {
    if let Err(e) = std::process::Command::new("sh")
        .args(["-c", &format!("setsid -f {command} >/dev/null 2>&1")])
        .spawn()
    {
        eprintln!("caelestia-launcher: cannot start {command}: {e}");
    }
}

/// Sizes the window to what the pane actually needs.
///
/// A layer surface anchored only at the top takes the window's own size, so
/// this is what makes the launcher grow and shrink with its results.
#[tauri::command]
fn resize(width: f64, height: f64, app: AppHandle) {
    let Some(window) = app.get_webview_window("launcher") else { return };
    // Clamped: a frontend bug must not ask for a surface taller than the
    // screen or too small to see.
    let size = tauri::LogicalSize::new(width.clamp(320.0, 2000.0), height.clamp(64.0, 1400.0));
    if let Err(e) = window.set_size(size) {
        eprintln!("caelestia-launcher: cannot resize: {e}");
    }
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
    show_with(app, "");
}

/// Opens with the search box already filled, so a keybind can drop straight
/// into a mode — `>wallpaper ` for the picker, `>calc ` for the calculator.
fn show_with(app: &AppHandle, query: &str) {
    let Some(window) = app.get_webview_window("launcher") else { return };

    // Anything could have changed while it sat hidden: a new app installed,
    // the wallpaper switched, the scheme changed from the shell.
    if let Some(state) = app.try_state::<Mutex<Launcher>>() {
        if let Ok(mut launcher) = state.lock() {
            launcher.refresh();
        }
    }
    // The frontend resets to this and refocuses, so the launcher never
    // reopens showing the last thing that was typed.
    let _ = app.emit("launcher-opened", query.to_string());
    let _ = window.show();
    let _ = window.set_focus();
}

fn toggle_with(app: &AppHandle, query: &str) {
    let Some(window) = app.get_webview_window("launcher") else { return };
    if window.is_visible().unwrap_or(false) {
        hide(app);
    } else {
        show_with(app, query);
    }
}

pub fn run() {
    // Checked before anything is built: a second launcher would cost another
    // webview, and the first one is the one the socket belongs to.
    if control::already_running() {
        eprintln!("caelestia-launcher: one is already running");
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![search, activate, open_in_calculator, resize, dismiss])
        .setup(|app| {
            let window = app.get_webview_window("launcher").expect("the launcher window exists");
            platform::place(&window);

            app.manage(Mutex::new(Launcher::new()));

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

    /// True when a launcher is already listening. Only a socket nothing
    /// answers on is safe to clear: deleting a live one leaves two launchers
    /// running, each holding its own webview.
    pub fn already_running() -> bool {
        UnixStream::connect(socket_path()).is_ok()
    }

    pub fn listen(app: AppHandle) {
        let path = socket_path();
        // Stale, from a launcher that crashed without cleaning up.
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
                // "show" and "toggle" may carry a starting query after a
                // space; everything up to the first space is the verb.
                let (verb, query) = word.split_once(' ').unwrap_or((word.trim(), ""));
                let (verb, query) = (verb.trim().to_string(), query.to_string());
                let _ = app.clone().run_on_main_thread(move || match verb.as_str() {
                    "show" => super::show_with(&app, &query),
                    "hide" => super::hide(&app),
                    _ => super::toggle_with(&app, &query),
                });
            }
        });
    }
}

pub use control::{already_running, send as send_control};

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

        if !gtk_layer_shell::is_supported() {
            eprintln!("caelestia-launcher: the compositor has no layer-shell; using a plain window");
            return place_plain(window);
        }

        // gtk-layer-shell has to get at the window before GTK realises it.
        // The window is declared `visible: false`, so Tauri builds it without
        // ever showing it and GTK leaves it unrealised until the first
        // `show()` — which is after this runs.
        gtk_window.init_layer_shell();
        if !gtk_window.is_layer_window() {
            eprintln!("caelestia-launcher: could not make the window a layer surface");
            return place_plain(window);
        }
        gtk_window.set_layer(Layer::Overlay);
        // Exclusive, or what is typed goes to whatever had focus before.
        gtk_window.set_keyboard_mode(KeyboardMode::Exclusive);
        // What the compositor's blur rule matches on.
        gtk_window.set_namespace("caelestia-launcher");

        // Anchored to the top and centred, a fifth of the way down: where the
        // eye already is, and clear of what is being searched over.
        gtk_window.set_anchor(Edge::Top, true);
        gtk_window.set_layer_shell_margin(Edge::Top, 220);
        for edge in [Edge::Left, Edge::Right, Edge::Bottom] {
            gtk_window.set_anchor(edge, false);
        }
        eprintln!(
            "caelestia-launcher: layer surface ready (protocol {})",
            gtk_layer_shell::protocol_version()
        );
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
