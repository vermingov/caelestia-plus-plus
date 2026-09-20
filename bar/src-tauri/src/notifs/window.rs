//! The surfaces the toasts and the notification centre are drawn on.
//!
//! One per output, down the right-hand edge, on the overlay layer — above a
//! fullscreen window, because whether a notification may appear over one is a
//! setting, and a surface underneath could never honour "yes".
//!
//! The surface is always mapped and almost always empty. It accepts the
//! pointer only where the page says something is drawn, so the rest of it is
//! not there as far as any window underneath is concerned.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, WebviewWindow};

use crate::Rect;

/// What the compositor's rules match on.
pub const NAMESPACE: &str = "caelestia-notifs";

/// Wide enough for the centre, which is the widest thing drawn here, plus the
/// room its shadow needs to fall off in.
const WIDTH: i32 = 460;

/// What `open_bars` pairs a window with: a GDK monitor where there is layer
/// shell to put it on one, and nothing at all where there is not.
#[cfg(feature = "layer-shell")]
pub type Monitor = gtk::gdk::Monitor;
#[cfg(not(feature = "layer-shell"))]
pub type Monitor = ();

pub fn label(output: &str) -> String {
    format!("notifs-{output}")
}

/// Builds the surface for one output.
///
/// Main thread only, like every window: it shares the bar's web process, and
/// the view that makes that possible cannot leave the thread it lives on.
pub fn open(app: &AppHandle, output: &str) -> Option<WebviewWindow> {
    let builder =
        tauri::WebviewWindowBuilder::new(app, label(output), tauri::WebviewUrl::App("notifs.html".into()))
            .title(NAMESPACE)
            .inner_size(f64::from(WIDTH), 1080.0)
            // Not resizable, for the reason the bars are not: a resizable
            // layer surface here maps, loads its page, and never paints.
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .visible(false)
            .skip_taskbar(true)
            .focused(false);

    match crate::sharing_web_process(builder, app).build() {
        Ok(window) => Some(window),
        Err(e) => {
            eprintln!("caelestia-bar: cannot open the notification surface for {output}: {e}");
            None
        }
    }
}

#[cfg(feature = "layer-shell")]
pub fn place(window: &WebviewWindow, monitor: Option<&Monitor>) {
    use gtk::prelude::*;
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    let Ok(gtk_window) = window.gtk_window() else { return };
    gtk_window.init_layer_shell();
    if !gtk_window.is_layer_window() {
        return;
    }
    gtk_window.set_layer(Layer::Overlay);
    // Nothing here is typed into, and a surface that took the keyboard would
    // take it from whatever the notification interrupted.
    gtk_window.set_keyboard_mode(KeyboardMode::None);
    gtk_window.set_namespace(NAMESPACE);

    // Top to bottom down the right edge. No exclusive zone of its own, so it
    // starts below the bar's rather than underneath it.
    for edge in [Edge::Top, Edge::Right, Edge::Bottom] {
        gtk_window.set_anchor(edge, true);
    }
    gtk_window.set_anchor(Edge::Left, false);
    if let Some(monitor) = monitor {
        gtk_window.set_monitor(monitor);
    }
    gtk_window.set_default_size(WIDTH, 1080);
    clip_input(&gtk_window, &[]);
}

/// Without layer shell it is an ordinary window kept on top. It cannot be
/// made to pass clicks through, so it is only shown while there is something
/// in it — see `set_reach`.
#[cfg(not(feature = "layer-shell"))]
pub fn place(window: &WebviewWindow, _monitor: Option<&Monitor>) {
    let _ = window.set_always_on_top(true);
}

/// Limits what accepts the pointer to the boxes the page has drawn.
#[cfg(feature = "layer-shell")]
fn clip_input(gtk_window: &gtk::ApplicationWindow, boxes: &[Rect]) {
    use gtk::cairo::{RectangleInt, Region};
    use gtk::prelude::*;

    let region = Region::create();
    for rect in boxes {
        let _ = region.union_rectangle(&RectangleInt::new(
            rect.x.max(0),
            rect.y.max(0),
            rect.width.max(1),
            rect.height.max(1),
        ));
    }
    gtk_window.input_shape_combine_region(Some(&region));
    // On Wayland the region is pending state that lands with the surface's
    // next commit, and GTK only commits when it paints. A page that has just
    // gone quiet may never paint again, which would leave the last toast's
    // box swallowing clicks long after the toast had gone.
    gtk_window.queue_draw();
}

/// GTK may only be touched from the thread its main loop runs on, and a
/// command handler is not it.
#[cfg(feature = "layer-shell")]
pub fn set_reach(window: &WebviewWindow, boxes: Vec<Rect>) {
    let handle = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(gtk_window) = handle.gtk_window() {
            clip_input(&gtk_window, &boxes);
        }
    });
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_reach(window: &WebviewWindow, boxes: Vec<Rect>) {
    let _ = if boxes.is_empty() { window.hide() } else { window.show() };
}

/// Where the pointer is, measured from this surface's own corner.
///
/// The page cannot be trusted to know: a pointer that leaves the input region
/// produces no event, so the last thing the page heard is that it was still
/// inside. The compositor is asked instead, as the bar does for its popouts.
pub fn pointer(window: &WebviewWindow, output: &str) -> Option<(i32, i32)> {
    let (x, y) = crate::hypr::cursor()?;
    let (left, top) = origin(window.label(), output)?;
    Some((x - left, y - top))
}

/// Where a surface sits, remembered for a few seconds: it moves only when an
/// output does, and asking costs a full listing of every layer.
fn origin(label: &str, output: &str) -> Option<(i32, i32)> {
    static ORIGINS: OnceLock<Mutex<HashMap<String, ((i32, i32), Instant)>>> = OnceLock::new();
    let mut origins = ORIGINS.get_or_init(Mutex::default).lock().ok()?;

    if let Some((origin, read)) = origins.get(label) {
        if read.elapsed() < Duration::from_secs(5) {
            return Some(*origin);
        }
    }
    let origin = crate::hypr::layer_origin_on(output, NAMESPACE)?;
    origins.insert(label.to_string(), (origin, Instant::now()));
    Some(origin)
}
