//! What cae looks like, as numbers.
//!
//! Near-black glass, one quiet rim, nothing coloured unless it is saying
//! something. These are the values the bar has always had; they lived in a
//! stylesheet when a webview drew it, and they are constants now that GPUI
//! does.

use std::sync::{Arc, LazyLock};

use gpui::{Background, BoxShadow, FontFeatures, Hsla, Pixels, hsla, linear_color_stop, linear_gradient, px, rgba};

/// The pill's own height, and the gap above it. The bar's surface is exactly
/// the two together: popouts are windows of their own, so nothing taller has
/// to be reserved for them.
pub const PILL: Pixels = px(38.);
pub const FLOAT: Pixels = px(12.);

pub const PILL_RADIUS: Pixels = px(8.);
/// A panel's corners: rounder than a pill's, because it is a bigger thing.
pub const RADIUS: Pixels = px(11.);
pub const GAP: Pixels = px(6.);

pub const FONT: &str = "Rubik";
pub const SYMBOLS: &str = "Material Symbols Rounded";

/// Figures all one width, for anything that counts. Rubik's are their natural
/// widths unless asked, and a one is two thirds of a zero: a clock set in
/// them shuffles sideways every minute, and a percentage drags its
/// neighbours about.
pub fn tabular() -> FontFeatures {
    static TABULAR: LazyLock<FontFeatures> = LazyLock::new(|| FontFeatures(Arc::new(vec![("tnum".to_string(), 1)])));
    TABULAR.clone()
}

/// White at some strength, which is what nearly everything here is.
pub fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

/// Black at some strength, for the one thing that darkens rather than
/// lightens: what is not being chosen from.
pub fn black(alpha: f32) -> Hsla {
    hsla(0., 0., 0., alpha)
}

pub fn text() -> Hsla {
    white(0.92)
}

pub fn text_dim() -> Hsla {
    white(0.56)
}

pub fn text_faint() -> Hsla {
    white(0.34)
}

/// The one colour, and it is the mark's own. Used for the logo, the focused
/// workspace and the dials, so that when it appears it means something.
pub fn accent() -> Hsla {
    rgba(0xff5449ff).into()
}

pub fn warn() -> Hsla {
    rgba(0xf5a25dff).into()
}

pub fn alert() -> Hsla {
    rgba(0xff8a80ff).into()
}

/// The ink on the accent: the focused workspace's number.
pub fn on_accent() -> Hsla {
    rgba(0x1a0d0cff).into()
}

/// The pill's surface. Denser than it would be over a compositor blur,
/// because there is none behind this surface: at this alpha it still takes
/// the colour of what is behind it without turning to soup over a busy
/// wallpaper.
pub fn face() -> Background {
    linear_gradient(180., linear_color_stop(rgba(0x1a1a1ed9), 0.), linear_color_stop(rgba(0x0d0d0fe0), 1.))
}

/// The pill's shadow and the hairline inside its edge.
pub fn face_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(6.), hsla(0., 0., 0., 0.45)).blur_radius(px(18.)).spread_radius(px(-8.)),
        BoxShadow::new(px(0.), px(0.), white(0.07)).spread_radius(px(1.)).inset(),
    ]
}

/// A trough that groups related readouts.
pub fn section_shadows() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), white(0.045)).spread_radius(px(1.)).inset()]
}

/// A panel's surface: the pill's own glass, lit from one corner.
pub fn panel() -> Background {
    linear_gradient(160., linear_color_stop(rgba(0x1a1a1ed9), 0.), linear_color_stop(rgba(0x0d0d0fe0), 0.6))
}

/// A panel's shadow, the hairline inside its edge, and the brighter one
/// along the top where the light catches it. Half-opaque at most, for the
/// reason `pane_shadows` gives.
pub fn panel_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(16.), hsla(0., 0., 0., 0.5)).blur_radius(px(36.)).spread_radius(px(-10.)),
        BoxShadow::new(px(0.), px(0.), white(0.055)).spread_radius(px(1.)).inset(),
        BoxShadow::new(px(0.), px(1.), white(0.07)).inset(),
    ]
}

/// A hairline drawn just inside an edge, in white at some strength: what
/// marks a filled thing off from the glass it sits on.
pub fn edge(alpha: f32) -> Vec<BoxShadow> {
    edge_in(white(alpha))
}

pub fn edge_in(colour: Hsla) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), colour).spread_radius(px(1.)).inset()]
}

/// What lifts a slider's thumb off its track.
pub fn thumb_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(1.), hsla(0., 0., 0., 0.5)).blur_radius(px(4.))]
}

/// The launcher's pane: the same glass as a panel's, thinner, because it is
/// over a blur the compositor makes for it and a panel is not.
pub fn pane() -> Background {
    linear_gradient(160., linear_color_stop(rgba(0x1a1a1eb8), 0.), linear_color_stop(rgba(0x0d0d0fc2), 0.42))
}

/// A window's surface: the same glass again, and the densest of the three.
/// A window is read for minutes rather than glanced at, over whatever happens
/// to be behind it, and with no promise of a blur: the compositor turns that
/// off on battery.
pub fn window() -> Background {
    linear_gradient(160., linear_color_stop(rgba(0x1a1a1eeb), 0.), linear_color_stop(rgba(0x0d0d0ff0), 0.5))
}

/// Depth under the pane, and the faintest lift along its top edge so that
/// the surface does not begin abruptly.
pub fn pane_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(20.), hsla(0., 0., 0., 0.5)).blur_radius(px(48.)).spread_radius(px(-10.)),
        BoxShadow::new(px(0.), px(4.), hsla(0., 0., 0., 0.42)).blur_radius(px(14.)).spread_radius(px(-4.)),
        BoxShadow::new(px(0.), px(0.), white(0.05)).spread_radius(px(1.)).inset(),
        BoxShadow::new(px(0.), px(1.), white(0.055)).inset(),
    ]
}

/// One half of the streak of light along the pane's top edge: up to its
/// brightest, or back down from it.
pub fn streak(rising: bool) -> Background {
    let (from, to) = if rising { (0., 0.16) } else { (0.16, 0.) };
    linear_gradient(90., linear_color_stop(white(from), 0.), linear_color_stop(white(to), 1.))
}

/// The chosen row: a smaller pane of the same glass, sitting on the first.
pub fn highlight() -> Background {
    linear_gradient(160., linear_color_stop(white(0.075), 0.), linear_color_stop(white(0.03), 1.))
}

pub fn highlight_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(2.), hsla(0., 0., 0., 0.5)).blur_radius(px(10.)).spread_radius(px(-4.)),
        BoxShadow::new(px(0.), px(0.), white(0.07)).spread_radius(px(1.)).inset(),
        BoxShadow::new(px(0.), px(1.), white(0.1)).inset(),
    ]
}

/// The chosen wallpaper, lifted off the strip.
pub fn tile_lifted() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.55)).blur_radius(px(22.)).spread_radius(px(-8.)),
        BoxShadow::new(px(0.), px(0.), white(0.16)).spread_radius(px(1.)).inset(),
    ]
}

/// A ring of one colour drawn round something, outside its edge: what makes
/// a small thing sitting on a picture read as cut out of it rather than
/// stuck on top.
pub fn ring(colour: Hsla, width: Pixels) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), colour).spread_radius(width)]
}

/// A panel's shadows for something that is raising its voice: the hairlines
/// in the alert colour, which is the one case a notification is given any.
pub fn critical_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(16.), hsla(0., 0., 0., 0.5)).blur_radius(px(36.)).spread_radius(px(-10.)),
        BoxShadow::new(px(0.), px(0.), rgba(0xff544952).into()).spread_radius(px(1.)).inset(),
        BoxShadow::new(px(0.), px(1.), rgba(0xff8a802e).into()).inset(),
    ]
}
