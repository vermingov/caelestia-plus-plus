//! The Caelestia++ bar: a layer surface across the top of the screen, drawn
//! by a webview and fed by the kernel and Hyprland.
//!
//! The shape of the thing is the same as the shell's own bar — logo, then
//! workspaces, then what is focused, then a spacer, then the readouts and the
//! clock — because that layout is not what was wrong with it.

mod children;
mod guards;
pub mod launcher;
mod hypr;
mod icons;
mod logo;
mod media;
#[cfg(feature = "layer-shell")]
mod repaint;
mod services;
mod spectrum;
mod startup;
mod system;
mod tray;
mod volume;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use tauri::WebviewWindow;

/// The pill itself, and the gap between it and the top of the screen.
///
/// The bar floats rather than spanning the output edge to edge, so what gets
/// reserved is the gap plus the pill — and the gap matches Hyprland's own
/// `gaps_out`, so the pill lines up with the tiles beneath it instead of
/// sitting at some margin of its own.
const PILL: i32 = 38;
// Twelve, not eight: the mark is 62px on a 38px pill, so it needs (62-38)/2
// of clearance above the pill or the top of the C is cut off by the edge of
// the screen — which reads as the logo sitting too low rather than as it
// being clipped.
const FLOAT: i32 = 12;

/// What the compositor is asked to keep clear.
const HEIGHT: i32 = PILL + FLOAT;

/// Wider than any output. Regions built with it are intersected with the
/// surface, whose width is decided by the anchors rather than here.
#[cfg(feature = "layer-shell")]
const ANY_WIDTH: i32 = 10_000;

/// How tall the *surface* is. Popouts are drawn inside it, so it has to be
/// tall enough for the largest of them without their having to scroll. It is
/// not free: the whole thing is a transparent buffer the compositor repaints,
/// so it is as tall as the panels need and no taller.
/// 620 rather than something snugger: the webview renders nothing at all at
/// some heights — 38 and 440 both came out blank, 620 does not — and a
/// surface that draws is worth more than the buffer saved by one that does
/// not. See `place` for the rest of this window's sizing folklore.
const SURFACE: i32 = 620;

/// How often the readouts are re-read. A second is the rate at which a CPU
/// percentage means anything; faster is a number that flickers rather than
/// one that informs.
const TICK: Duration = Duration::from_secs(1);

/// The slow tick. Everything on it costs a process, so it runs at a rate a
/// person would notice a stale reading at rather than at the rate the numbers
/// could change.
const SLOW_TICK: Duration = Duration::from_secs(5);

/// Why nearly every command below says `(async)`.
///
/// Tauri runs a synchronous command on the main thread — the same thread the
/// GTK main loop and the webview's compositor live on. Every one of these
/// asks something outside the process: a hyprctl round trip, `nmcli`,
/// `bluetoothctl`, a DBus call to a tray app. `pointer` alone runs five times
/// a second for as long as the pointer is over a popout, and `wifi_join`
/// blocks until the association finishes or gives up.
///
/// On the main thread that is not slow, it is *frozen*: no frame, no hover,
/// no click, for as long as the other program takes. `(async)` puts the
/// command on the async runtime instead, where blocking costs a worker thread
/// and nothing anybody can see. Only the handful that build, show or measure
/// a window stay synchronous, because those genuinely have to be on the
/// thread that owns the window.

/// Which mark the bar wears. Read on demand: it changes when a person edits
/// their config, not on a tick.
#[tauri::command(async)]
fn logo() -> logo::Logo {
    logo::read()
}

/// The bar's own options, read from shell.json.
#[tauri::command(async)]
fn bar_config() -> logo::BarConfig {
    logo::bar_config()
}

/// What the bar is made of and in what order, which is the user's config.
#[tauri::command(async)]
fn layout() -> logo::Layout {
    logo::layout()
}

/// The pointer's position inside this window, in the page's own coordinates.
///
/// The front end uses it to decide whether a popout has been left, because
/// the events and the hover state that would normally answer that are both
/// unreliable once the pointer crosses out of the surface's input region.
/// Returns nothing when the compositor cannot be asked.
#[tauri::command(async)]
fn pointer() -> Option<(i32, i32)> {
    let (x, y) = hypr::cursor()?;
    let (left, top) = surface_origin();
    Some((x - left, y - top))
}

/// Where the bar's surface sits, cached.
///
/// It moves only when something changes an exclusive zone, and asking costs a
/// full layer listing — which is not a thing to do five times a second while
/// somebody holds the pointer over a popout.
fn surface_origin() -> (i32, i32) {
    use std::sync::OnceLock;
    static ORIGIN: OnceLock<Mutex<((i32, i32), std::time::Instant)>> = OnceLock::new();

    let cache = ORIGIN.get_or_init(|| {
        Mutex::new((
            hypr::layer_origin("caelestia-bar").unwrap_or((0, 0)),
            std::time::Instant::now(),
        ))
    });

    let Ok(mut cache) = cache.lock() else { return (0, 0) };
    if cache.1.elapsed() > Duration::from_secs(5) {
        if let Some(origin) = hypr::layer_origin("caelestia-bar") {
            cache.0 = origin;
        }
        cache.1 = std::time::Instant::now();
    }
    cache.0
}

#[tauri::command(async)]
fn state() -> hypr::State {
    hypr::read_state()
}

/// The three pushed feeds, as they stand right now.
///
/// Each of them only emits on a change, and their first change happens while
/// the webview is still starting — so a front end that has just subscribed
/// asks once rather than waiting for the next one.
#[tauri::command(async)]
fn snapshot(
    watcher: tauri::State<'_, guards::Watcher>,
) -> (Vec<tray::Item>, services::Snapshot, Option<media::NowPlaying>) {
    (tray::items(), services::read(watcher.read()), media::now())
}

#[tauri::command(async)]
fn focus_workspace(id: i64) {
    hypr::dispatch(&format!("workspace {id}"));
}

#[tauri::command(async)]
fn toggle_special(name: String) {
    hypr::toggle_special(&name);
}

#[tauri::command(async)]
fn cycle_workspace(forward: bool) {
    hypr::dispatch(if forward { "workspace r+1" } else { "workspace r-1" });
}

#[tauri::command(async)]
fn volume(delta: i64) {
    volume::nudge(delta);
}

#[tauri::command(async)]
fn volume_to(level: i64) {
    volume::set(level);
}

/// What of the surface accepts the pointer.
///
/// The bar is a strip at the top of a much taller surface, and everything
/// below the strip belongs to whatever window is under it — until a popout
/// opens, which needs the pointer to be able to reach it. Only the popout's
/// own box is added, not the whole width of the screen down to its bottom
/// edge: a panel two hundred pixels wide must not make the rest of the row
/// unclickable.
#[tauri::command]
fn reach(popout: Option<Rect>, window: tauri::WebviewWindow) {
    set_reach(&window, popout);
}

/// Whether the page is drawing anything below the strip: a popout, a tray
/// menu, either of them on its way in or out.
///
/// Not the same thing as `reach`, which ends when the pointer can no longer
/// use a popout. That is when it starts to leave, and it is still being drawn
/// for as long as leaving takes.
#[tauri::command]
fn overhang(open: bool, window: tauri::WebviewWindow) {
    set_overhang(&window, open);
}

/// A popout's box in surface coordinates, as the front end measured it.
#[derive(Clone, Copy, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[tauri::command(async)]
fn mute() {
    volume::toggle_mute();
}

#[tauri::command(async)]
fn mic_mute() {
    volume::toggle_microphone();
}

#[tauri::command(async)]
fn mic_to(level: i64) {
    volume::set_microphone(level);
}

#[tauri::command(async)]
fn media_control(action: String) {
    media::control(&action);
}

#[tauri::command(async)]
fn tray_activate(key: String, x: i32, y: i32) {
    tray::activate(&key, x, y);
}

#[tauri::command(async)]
fn tray_secondary(key: String, x: i32, y: i32) {
    tray::secondary_activate(&key, x, y);
}

#[tauri::command(async)]
fn tray_menu(key: String) -> Vec<tray::MenuEntry> {
    tray::menu(&key)
}

#[tauri::command(async)]
fn tray_click(key: String, id: i32) {
    tray::click(&key, id);
}

#[tauri::command(async)]
fn brightness(delta: i64) {
    system::nudge_brightness(delta);
}

#[tauri::command(async)]
fn brightness_to(level: i64) {
    system::set_brightness(level);
}

#[tauri::command(async)]
fn power_profile(profile: String) {
    services::set_power_profile(&profile);
}

/// Opens the shell's security centre on whichever tab is asking.
#[tauri::command(async)]
fn dynamic_profile(on: bool) {
    services::set_dynamic(on);
}

/// Opens the shell's nexus, which is where its detached panels live — the
/// "open settings" the shell's own popouts offer.
#[tauri::command(async)]
fn open_settings() {
    services::ipc("nexus", "open", &[]);
}

/// Opens the panel — the security centre and the feature menu, which are the
/// bar's own windows now rather than the shell's.
#[tauri::command]
fn security(tab: String, app: AppHandle) {
    open_panel(&app, if tab == "protection" { "protection" } else { "firewall" });
}

#[tauri::command]
fn close_panel(app: AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
}

/// Which tab the panel should land on, for a window that has just loaded.
#[tauri::command]
fn panel_tab(tab: tauri::State<'_, Mutex<String>>) -> String {
    tab.lock().map(|tab| tab.clone()).unwrap_or_default()
}

#[tauri::command(async)]
fn startup_list() -> Vec<startup::Entry> {
    startup::scan()
}

#[tauri::command(async)]
fn startup_set(source: String, key: String, enabled: bool) {
    startup::set_enabled(&source, &key, enabled);
}

#[tauri::command(async)]
fn startup_remove(source: String, key: String) {
    startup::remove(&source, &key);
}

#[tauri::command(async)]
fn startup_add(name: String, exec: String) {
    startup::add(&name, &exec);
}

#[tauri::command(async)]
fn guards_detail(watcher: tauri::State<'_, guards::Watcher>) -> Vec<guards::Detail> {
    watcher.detail()
}

#[tauri::command(async)]
fn guards_verdict(
    which: String,
    id: i64,
    action: String,
    remember: bool,
    watcher: tauri::State<'_, guards::Watcher>,
) {
    watcher.verdict(&which, id, &action, remember);
}

#[tauri::command(async)]
fn guards_set_rule(
    which: String,
    exe: String,
    action: String,
    name: String,
    watcher: tauri::State<'_, guards::Watcher>,
) {
    watcher.set_rule(&which, &exe, &action, &name);
}

#[tauri::command(async)]
fn guards_delete_rule(which: String, exe: String, watcher: tauri::State<'_, guards::Watcher>) {
    watcher.delete_rule(&which, &exe);
}

#[tauri::command(async)]
fn guards_set_enabled(which: String, enabled: bool, watcher: tauri::State<'_, guards::Watcher>) {
    watcher.set_enabled(&which, enabled);
}

/// Flips one feature mode by id, through the shell's own hub — the modes it
/// owns persist to disk and some install a privileged half on first use, none
/// of which belongs in a bar.
#[tauri::command(async)]
fn feature_toggle(id: String) {
    services::ipc("features", "toggle", &[&id]);
}

#[tauri::command]
fn features_menu(app: AppHandle) {
    open_panel(&app, "features");
}

/// The session menu is one of the shell's drawers, not a thing of ours.
#[tauri::command(async)]
fn session() {
    services::ipc("drawers", "toggle", &["session"]);
}

#[tauri::command(async)]
fn wifi_list() -> Vec<services::Wifi> {
    services::networks()
}

#[tauri::command(async)]
fn wifi_join(ssid: String, password: String) -> Result<(), String> {
    services::join(&ssid, &password)
}

#[tauri::command(async)]
fn wifi_radio(on: bool) {
    services::set_wifi(on);
}

#[tauri::command(async)]
fn wifi_rescan() {
    services::rescan();
}

#[tauri::command(async)]
fn ethernet_list() -> Vec<services::Ethernet> {
    services::ethernet()
}

#[tauri::command(async)]
fn ethernet_set(interface: String, connect: bool) {
    services::set_ethernet(&interface, connect);
}

#[tauri::command(async)]
fn bluetooth_discover(on: bool) {
    services::set_discovering(on);
}

#[tauri::command(async)]
fn bluetooth_forget(address: String) {
    services::forget_device(&address);
}

#[tauri::command(async)]
fn audio_nodes() -> (Vec<services::AudioNode>, Vec<services::AudioNode>) {
    (services::sinks(), services::sources())
}

#[tauri::command(async)]
fn audio_default(kind: String, name: String) {
    services::set_default_node(&kind, &name);
}

#[tauri::command(async)]
fn bed_mode() {
    services::toggle_bed_mode();
}

#[tauri::command(async)]
fn bluetooth_devices() -> Vec<services::Device> {
    services::devices()
}

#[tauri::command(async)]
fn bluetooth_radio(on: bool) {
    services::set_bluetooth(on);
}

#[tauri::command(async)]
fn bluetooth_connect(address: String, connect: bool) {
    services::connect_device(&address, connect);
}

/// Opens the launcher, which is its own process and its own surface.
#[tauri::command]
fn toggle_launcher(app: AppHandle) {
    launcher::toggle_with(&app, "");
}

/// Whether to narrate what the windows are doing.
///
/// Off unless CAELESTIA_BAR_DIAG is set, so a release build pays for one
/// environment lookup per window at startup and nothing after.
fn diagnosing() -> bool {
    std::env::var_os("CAELESTIA_BAR_DIAG").is_some()
}

/// Somewhere for the front end to report what went wrong.
///
/// A webview that fails while starting up renders nothing and says nothing:
/// the window is transparent, so a broken one and an idle one look identical
/// from outside. Costs nothing unless something calls it, and what calls it
/// is error paths.
#[tauri::command]
fn diag(window: WebviewWindow, message: String) {
    if diagnosing() {
        eprintln!("caelestia-bar[{}]: {message}", window.label());
    }
}

#[tauri::command(async)]
fn run(command: String) {
    let _ = std::process::Command::new("sh")
        .args(["-c", &format!("setsid -f {command} >/dev/null 2>&1")])
        .spawn();
}

pub fn start() {
    tauri::Builder::default()
        // The two guards push over their own sockets, so their state is
        // already current by the time anything asks for it.
        .manage(guards::Watcher::start())
        .manage(Mutex::new(Outputs::default()))
        // Which tab the panel was last asked for, so a window that has only
        // just loaded knows where it is meant to land.
        .manage(Mutex::new(String::from("firewall")))
        .invoke_handler(tauri::generate_handler![
            state,
            pointer,
            snapshot,
            logo,
            bar_config,
            layout,
            monitor,
            focus_workspace,
            toggle_special,
            cycle_workspace,
            volume,
            volume_to,
            mute,
            mic_mute,
            mic_to,
            brightness,
            brightness_to,
            media_control,
            tray_activate,
            tray_secondary,
            tray_menu,
            tray_click,
            power_profile,
            dynamic_profile,
            open_settings,
            security,
            features_menu,
            close_panel,
            panel_tab,
            startup_list,
            startup_set,
            startup_remove,
            startup_add,
            guards_detail,
            guards_verdict,
            guards_set_rule,
            guards_delete_rule,
            guards_set_enabled,
            feature_toggle,
            session,
            wifi_list,
            wifi_join,
            wifi_radio,
            wifi_rescan,
            ethernet_list,
            ethernet_set,
            bluetooth_discover,
            bluetooth_forget,
            audio_nodes,
            audio_default,
            bed_mode,
            bluetooth_devices,
            bluetooth_radio,
            bluetooth_connect,
            reach,
            overhang,
            toggle_launcher,
            run,
            diag,
            launcher::search,
            launcher::activate,
            launcher::open_in_calculator,
            launcher::dismiss
        ])
        .setup(|app| {
            for (window, monitor) in open_bars(app.handle()) {
                place(&window, monitor);
                trim_webview(&window);
                window.show()?;
                // Again now the surface is realised: an input region set on
                // an unrealised GTK window does not survive being mapped, and
                // the bar ends up accepting nothing at all.
                set_reach(&window, None);
            }

            launcher::setup(app.handle());

            watch_hyprland(app.handle().clone());
            sample_system(app.handle().clone());
            sample_services(app.handle().clone());
            watch_tray(app.handle().clone());
            watch_config(app.handle().clone());
            watch_spectrum(app.handle().clone());
            watch_media(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the bar could not start");
}

/// Turns off the parts of the webview a bar has no use for.
///
/// A WebKit web process starts at well over a hundred megabytes, and a fair
/// slice of that is machinery for being a browser: a back/forward page cache,
/// offline application storage, WebGL, media playback and capture, developer
/// tooling. None of it is reachable from a page that is a strip of glyphs
/// served from the binary, and all of it is allocated whether or not it is
/// used.
fn trim_webview(window: &WebviewWindow) {
    use webkit2gtk::{SettingsExt, WebViewExt};

    let _ = window.with_webview(|webview| {
        let view = webview.inner();
        let Some(settings) = WebViewExt::settings(&view) else { return };

        // Nothing here navigates, so there is nothing to keep a page cache of.
        settings.set_enable_page_cache(false);
        settings.set_enable_back_forward_navigation_gestures(false);

        // No plugins, no Java, no offline app storage, no HTML5 database.
        settings.set_enable_html5_database(false);
        settings.set_enable_html5_local_storage(false);
        settings.set_enable_offline_web_application_cache(false);

        // The bar draws with 2D canvas and CSS. WebGL and its whole GL stack
        // is the single largest thing switched off here.
        settings.set_enable_webgl(false);
        settings.set_enable_webaudio(false);

        // It has no video, no microphone and no camera.
        settings.set_enable_media(false);
        settings.set_enable_media_stream(false);
        settings.set_enable_mediasource(false);
        settings.set_enable_encrypted_media(false);

        // Nothing is typed into it, so spell checking has nothing to check,
        // and nothing is printed from it.
        settings.set_enable_developer_extras(false);
        settings.set_enable_javascript_markup(true);
    });
}

/// Puts a window that is about to be built in the first bar's web process.
///
/// WebKit gives every view a web process of its own unless it is told the
/// view is related to one it already has. The launcher and the panel are
/// hidden for nearly all of a session, and each was most of a hundred
/// megabytes of process kept alive to be hidden in.
///
/// Main thread only, which is where windows are built. The view is a GTK
/// object and cannot be handed between threads, so it is left in a slot on
/// this one — and `with_webview` only runs before it returns when it is called
/// from the thread it would otherwise have posted to.
fn sharing_web_process<'a>(
    builder: tauri::WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
    app: &AppHandle,
) -> tauri::WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    thread_local! {
        static FIRST: std::cell::RefCell<Option<webkit2gtk::WebView>> =
            const { std::cell::RefCell::new(None) };
    }

    let Some(bar) = app.get_webview_window("bar") else { return builder };
    let _ = bar.with_webview(|webview| FIRST.with(|first| *first.borrow_mut() = Some(webview.inner())));
    match FIRST.with(|first| first.borrow_mut().take()) {
        Some(view) => builder.with_related_view(view),
        None => builder,
    }
}

/// The panel's window label.
const PANEL: &str = "panel";

/// Opens the security centre and feature menu on a given tab, building the
/// window the first time it is asked for.
///
/// Built lazily rather than at startup: it is a window most sessions never
/// open, and an idle webview is fifty megabytes that nobody asked for.
fn open_panel(app: &AppHandle, tab: &str) {
    if let Ok(mut current) = app.state::<Mutex<String>>().lock() {
        *current = tab.to_string();
    }

    if let Some(panel) = app.get_webview_window(PANEL) {
        // Already open on this tab: the same click that opened it closes it,
        // which is how the shell's own shield and wrench behave.
        if panel.is_visible().unwrap_or(false) {
            let _ = app.emit_to(PANEL, "panel-tab", tab);
            let _ = panel.hide();
            return;
        }
        let _ = app.emit_to(PANEL, "panel-tab", tab);
        let _ = panel.show();
        let _ = panel.set_focus();
        return;
    }

    let builder = tauri::WebviewWindowBuilder::new(app, PANEL, tauri::WebviewUrl::App("panel.html".into()))
        .title("caelestia-panel")
        .inner_size(1920.0, 1080.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .visible(false)
        .skip_taskbar(true);
    let built = sharing_web_process(builder, app).build();

    match built {
        Ok(panel) => {
            place_panel(&panel);
            let _ = panel.show();
            let _ = panel.set_focus();
        }
        Err(e) => eprintln!("caelestia-bar: cannot open the panel: {e}"),
    }
}

/// The panel is a full-screen overlay: it takes the keyboard, dims nothing,
/// and closes when you click past it.
#[cfg(feature = "layer-shell")]
fn place_panel(window: &WebviewWindow) {
    use gtk::prelude::*;
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    let Ok(gtk_window) = window.gtk_window() else { return };
    gtk_window.init_layer_shell();
    if !gtk_window.is_layer_window() {
        return;
    }
    gtk_window.set_layer(Layer::Overlay);
    // Unlike the bar, this one is read and typed into.
    gtk_window.set_keyboard_mode(KeyboardMode::Exclusive);
    gtk_window.set_namespace("caelestia-panel");
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        gtk_window.set_anchor(edge, true);
    }
}

#[cfg(not(feature = "layer-shell"))]
fn place_panel(window: &WebviewWindow) {
    let _ = window.set_always_on_top(true);
    let _ = window.set_decorations(false);
}

/// One bar per output, each paired with the monitor it belongs on.
///
/// The window declared in the config takes the first output; the rest are
/// built here. Their labels carry the output's name so that a window can be
/// asked which screen it is on.
#[cfg(feature = "layer-shell")]
fn open_bars(app: &AppHandle) -> Vec<(WebviewWindow, Option<gtk::gdk::Monitor>)> {
    use gtk::prelude::*;

    let outputs = hypr::monitors();
    let display = gtk::gdk::Display::default();
    let count = display.as_ref().map(|display| display.n_monitors()).unwrap_or(1).max(1);

    let mut bars = Vec::new();
    for index in 0..count {
        let monitor = display.as_ref().and_then(|display| display.monitor(index));

        // GDK numbers its monitors and Hyprland names them; the geometry is
        // the only thing both report, so that is what pairs them up.
        let name = monitor
            .as_ref()
            .map(gtk::gdk::Monitor::geometry)
            .and_then(|area| {
                outputs
                    .iter()
                    .find(|(_, x, y, width, height)| {
                        *x == area.x() && *y == area.y() && *width == area.width() && *height == area.height()
                    })
                    .map(|(name, ..)| name.clone())
            })
            .unwrap_or_else(|| format!("output-{index}"));

        let label = if index == 0 { "bar".to_string() } else { format!("bar-{name}") };
        let window = match app.get_webview_window(&label) {
            Some(window) => window,
            None => {
                let builder = tauri::WebviewWindowBuilder::new(
                    app,
                    &label,
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("caelestia-bar")
                .inner_size(1920.0, f64::from(SURFACE))
                // Not resizable, which is what the window in the config is.
                // A resizable one renders nothing at all here: the surface is
                // mapped, the page loads and runs, and no frame is ever
                // painted — so on a single-monitor machine, where this window
                // is the only one built by hand, the bug is invisible and on
                // a three-monitor desktop two screens have no bar.
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .visible(false)
                .skip_taskbar(true)
                .focused(false)
                .on_page_load(|window, payload| {
                    if diagnosing() {
                        eprintln!(
                            "caelestia-bar[{}]: page {:?} {}",
                            window.label(),
                            payload.event(),
                            payload.url()
                        );
                    }
                });

                match sharing_web_process(builder, app).build() {
                    Ok(window) => window,
                    Err(e) => {
                        eprintln!("caelestia-bar: cannot open a bar for {name}: {e}");
                        continue;
                    }
                }
            }
        };

        if diagnosing() {
            eprintln!(
                "caelestia-bar[{label}]: output {name}, monitor {}",
                monitor.as_ref().map_or("none".to_string(), |m| {
                    let area = gtk::prelude::MonitorExt::geometry(m);
                    format!("{}x{} at {},{}", area.width(), area.height(), area.x(), area.y())
                })
            );
        }
        if let Ok(mut outputs) = app.state::<Mutex<Outputs>>().lock() {
            outputs.0.insert(label, name);
        }
        bars.push((window, monitor));
    }
    bars
}

/// Without layer shell there is one window and no way to place it, so there
/// is one bar.
#[cfg(not(feature = "layer-shell"))]
fn open_bars(app: &AppHandle) -> Vec<(WebviewWindow, Option<()>)> {
    app.get_webview_window("bar").map(|window| (window, None)).into_iter().collect()
}

/// Which output each bar window is on, by window label.
#[derive(Default)]
struct Outputs(std::collections::HashMap<String, String>);

/// The output this window is drawn on, so its workspace row can show that
/// screen's workspaces rather than the focused screen's.
#[tauri::command]
fn monitor(window: WebviewWindow, outputs: tauri::State<'_, Mutex<Outputs>>) -> String {
    outputs
        .lock()
        .ok()
        .and_then(|outputs| outputs.0.get(window.label()).cloned())
        .unwrap_or_default()
}

/// Hyprland's own event stream, on its own thread. It blocks between events,
/// so it costs nothing while nothing is happening — which is the difference
/// between this and polling for the focused workspace.
fn watch_hyprland(app: AppHandle) {
    std::thread::spawn(move || {
        // Outputs come and go — a laptop is docked, a projector is plugged in
        // — and each one needs its own bar. The count is checked on every
        // change rather than only at startup.
        let mut outputs = hypr::monitors().len();
        let mut view = None;
        hypr::watch(move |state| {
            // Another workspace coming to the front is the person looking
            // away, and the launcher cannot notice that for itself.
            if view.as_ref().is_some_and(|before| *before != state.view) {
                let app = app.clone();
                let _ = app.clone().run_on_main_thread(move || launcher::close_if_open(&app));
            }
            view = Some(state.view.clone());

            let _ = app.emit("hypr", state);

            let now = hypr::monitors().len();
            if now != outputs {
                outputs = now;
                rebuild_bars(&app);
            }
        });
    });
}

/// Opens a bar for any output that has gained one and closes any whose output
/// has gone. Runs on the Hyprland watcher's thread, and the window work is
/// handed to the main one because GTK insists.
fn rebuild_bars(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let live: Vec<String> = hypr::monitors().into_iter().map(|(name, ..)| name).collect();

        // Anything whose output is no longer there.
        let stale: Vec<(String, String)> = app
            .state::<Mutex<Outputs>>()
            .lock()
            .map(|outputs| {
                outputs
                    .0
                    .iter()
                    .filter(|(_, output)| !live.contains(output))
                    .map(|(label, output)| (label.clone(), output.clone()))
                    .collect()
            })
            .unwrap_or_default();

        for (label, output) in stale {
            // The window declared in the config is kept and re-placed rather
            // than destroyed: it is the one the app was built around.
            if label != "bar" {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.close();
                }
            }
            if let Ok(mut outputs) = app.state::<Mutex<Outputs>>().lock() {
                outputs.0.remove(&label);
            }
            eprintln!("caelestia-bar: {output} went away");
        }

        for (window, monitor) in open_bars(&app) {
            // Already up on its own output: leave it alone.
            if window.is_visible().unwrap_or(false) {
                continue;
            }
            place(&window, monitor);
            let _ = window.show();
            set_reach(&window, None);
        }
    });
}

/// The readouts, once a second, and the volume the moment it moves.
fn sample_system(app: AppHandle) {
    let (moved, moves) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        volume::watch(|levels| {
            let _ = moved.send(levels);
        });
    });

    std::thread::spawn(move || {
        let mut sampler = system::Sampler::new();
        let mut levels = volume::Levels::default();
        let mut shown: Option<system::Snapshot> = None;
        let mut due = Instant::now() + TICK;

        loop {
            let snapshot = match moves.recv_timeout(due.saturating_duration_since(Instant::now())) {
                // Only the volume is new, and the rest stays as it was read:
                // a CPU percentage averaged over the moment since a volume
                // key was pressed is noise.
                Ok(now) => {
                    levels = now;
                    let Some(shown) = &shown else { continue };
                    system::Snapshot {
                        volume: levels.volume.clone(),
                        microphone: levels.microphone.clone(),
                        ..shown.clone()
                    }
                }
                Err(_) => {
                    // A watcher that has gone lands here at once rather than
                    // on time, and must not turn the tick into a spin.
                    std::thread::sleep(due.saturating_duration_since(Instant::now()));
                    due = Instant::now() + TICK;
                    sampler.sample(&levels)
                }
            };

            // Nothing to draw differently, nothing to wake the webview for.
            // At rest this is most of the ticks.
            if shown.as_ref() == Some(&snapshot) {
                continue;
            }
            shown = Some(snapshot.clone());
            let _ = app.emit("system", snapshot);
        }
    });
}

/// The visualiser, on its own thread because it blocks on a pipe of audio.
///
/// Frames are emitted as they come, but only while something is playing: the
/// analyser sends one flat frame when sound stops and then nothing at all, so
/// a quiet desktop repaints nothing.
fn watch_spectrum(app: AppHandle) {
    std::thread::spawn(move || {
        spectrum::watch(|bars, live| {
            let _ = app.emit("spectrum", (bars, live));
        });
    });
}

/// What is playing, over MPRIS.
fn watch_media(app: AppHandle) {
    std::thread::spawn(move || {
        media::watch(|now_playing| {
            let _ = app.emit("media", now_playing);
        });
    });
}

/// Re-reads the settings when somebody changes them.
///
/// Everything the bar draws from config — the mark, the entry list, the bar's
/// own options — used to be read once at startup and never again, so turning
/// the logo off in Settings wrote the preference and changed nothing on
/// screen until the bar was restarted.
fn watch_config(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = logo::stamp();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let now = logo::stamp();
            if now == last {
                continue;
            }
            last = now;
            let _ = app.emit("config", (logo::read(), logo::bar_config(), logo::layout()));
        }
    });
}

/// The tray, on its own thread because it holds a DBus connection open.
fn watch_tray(app: AppHandle) {
    std::thread::spawn(move || {
        tray::watch(|items| {
            let _ = app.emit("tray", items);
        });
    });
}

/// The slow feed: power profile, the two guards, feature modes, bluetooth.
///
/// Separate from the fast one because it is the expensive half, and separate
/// from the front end because a bar that shells out on every frame is the
/// thing this whole port was meant to stop being.
fn sample_services(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            let snapshot = services::read(app.state::<guards::Watcher>().read());
            if last.as_ref() != Some(&snapshot) {
                last = Some(snapshot.clone());
                let _ = app.emit("services", snapshot);
            }
            std::thread::sleep(SLOW_TICK);
        }
    });
}

#[cfg(feature = "layer-shell")]
fn place(window: &WebviewWindow, monitor: Option<gtk::gdk::Monitor>) {
    use gtk::prelude::*;
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    let Ok(gtk_window) = window.gtk_window() else {
        eprintln!("caelestia-bar: no GTK window to place");
        return;
    };

    // Before the window is ever realised, which is why it is declared
    // invisible in the config and shown from `setup`.
    gtk_window.init_layer_shell();
    if !gtk_window.is_layer_window() {
        eprintln!("caelestia-bar: could not make the window a layer surface");
        return;
    }
    gtk_window.set_layer(Layer::Top);
    // A bar is not a place to type: taking the keyboard would swallow every
    // shortcut the moment the pointer crossed it.
    gtk_window.set_keyboard_mode(KeyboardMode::None);
    gtk_window.set_namespace("caelestia-bar");

    // Across the top, both corners anchored so it spans whatever the output
    // is rather than a width guessed here.
    for edge in [Edge::Top, Edge::Left, Edge::Right] {
        gtk_window.set_anchor(edge, true);
    }
    gtk_window.set_anchor(Edge::Bottom, false);
    gtk_window.set_exclusive_zone(HEIGHT);

    // One output each. Without this every surface lands on whichever monitor
    // the compositor feels like, which on a multi-head desktop means two bars
    // stacked on one screen and none on the other.
    if let Some(monitor) = monitor {
        gtk_window.set_monitor(&monitor);
    }

    // The surface is as tall as the popouts need; see SURFACE. The width is a
    // placeholder, because anchoring to both side edges overrides it.
    gtk_window.set_default_size(1920, SURFACE);
    clip_input(&gtk_window, None);
    repaint::begin(&gtk_window);
}

/// Limits the surface's input region to the top `height` pixels of it.
///
/// Without this the whole surface is clickable, and the part of it below the
/// bar is both invisible and in front of everything else.
#[cfg(feature = "layer-shell")]
fn clip_input(gtk_window: &gtk::ApplicationWindow, popout: Option<Rect>) {
    use gtk::cairo::{RectangleInt, Region};
    use gtk::prelude::*;

    let region = Region::create_rectangle(&RectangleInt::new(0, 0, ANY_WIDTH, HEIGHT));
    if let Some(rect) = popout {
        region.union_rectangle(&RectangleInt::new(
            rect.x.max(0),
            rect.y.max(0),
            rect.width.max(1),
            rect.height.max(1),
        )).ok();
    }
    gtk_window.input_shape_combine_region(Some(&region));
}

/// Re-clips the input region. GTK may only be touched from the thread its
/// main loop runs on, and a command handler is not it.
#[cfg(feature = "layer-shell")]
fn set_reach(window: &tauri::WebviewWindow, popout: Option<Rect>) {
    let window = window.clone();
    let _ = window.clone().run_on_main_thread(move || {
        if let Ok(gtk_window) = window.gtk_window() {
            clip_input(&gtk_window, popout);
        }
    });
}

/// Without layer shell there is no input region to clip: the window is an
/// ordinary one and is already only as big as itself.
#[cfg(not(feature = "layer-shell"))]
fn set_reach(_window: &tauri::WebviewWindow, _popout: Option<Rect>) {}

/// Tells the surface how much of itself a frame has to repaint, on the GTK
/// thread for the same reason as `set_reach`.
#[cfg(feature = "layer-shell")]
fn set_overhang(window: &tauri::WebviewWindow, open: bool) {
    let window = window.clone();
    let _ = window.clone().run_on_main_thread(move || {
        if let Ok(gtk_window) = window.gtk_window() {
            repaint::follow(&gtk_window, open);
        }
    });
}

/// And nothing below the strip to leave unpainted either.
#[cfg(not(feature = "layer-shell"))]
fn set_overhang(_window: &tauri::WebviewWindow, _open: bool) {}

#[cfg(not(feature = "layer-shell"))]
fn place(window: &WebviewWindow, _monitor: Option<()>) {
    // No layer shell: an always-on-top strip is the closest thing available,
    // and it will not reserve space.
    let _ = window.set_always_on_top(true);
    let _ = window.set_decorations(false);
}
