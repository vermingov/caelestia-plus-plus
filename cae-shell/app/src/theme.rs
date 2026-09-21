//! What cae looks like, as numbers.
//!
//! Near-black glass, one quiet rim, nothing coloured unless it is saying
//! something. The glass is a constant; the colour is not. What little of the
//! shell is coloured comes from the scheme the person has set, which the CLI
//! keeps in `scheme.json` and rewrites every themed config from — the bar
//! being one more thing themed by it rather than a thing with a colour of
//! its own.
//!
//! These began as the hex the stylesheet had when a webview drew the bar,
//! and the hex was whatever the scheme happened to be on the machine it was
//! written on. Anybody else's shell was that machine's red for ever.

use std::sync::{Arc, LazyLock, RwLock};

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

/// What the scheme says, or what the shell was written with.
///
/// Read once and kept, because a colour is asked for many times a frame and
/// the answer only changes when somebody picks a new scheme. The file is
/// looked at again at most a few times a second, which is far faster than
/// anyone can choose a colour and far cheaper than a read per quad.
fn coloured(key: &str, fallback: u32) -> Hsla {
    static KEPT: LazyLock<RwLock<(std::time::Instant, Arc<Palette>)>> =
        LazyLock::new(|| RwLock::new((std::time::Instant::now(), Arc::new(Palette::read()))));
    const STALE: std::time::Duration = std::time::Duration::from_millis(400);

    let palette = {
        let kept = KEPT.read().unwrap_or_else(|held| held.into_inner());
        if kept.0.elapsed() < STALE {
            Some(Arc::clone(&kept.1))
        } else {
            None
        }
    };
    let palette = palette.unwrap_or_else(|| {
        let fresh = Arc::new(Palette::read());
        if let Ok(mut kept) = KEPT.write() {
            *kept = (std::time::Instant::now(), Arc::clone(&fresh));
        }
        fresh
    });

    palette.0.get(key).copied().map_or_else(|| rgba(fallback).into(), |colour| rgba(colour).into())
}

/// The scheme's colours, by the names the CLI writes them under.
struct Palette(std::collections::HashMap<String, u32>);

impl Palette {
    fn read() -> Palette {
        Palette(
            cae_core::scheme::colours()
                .into_iter()
                .filter_map(|(name, hex)| {
                    let hex = hex.strip_prefix('#').unwrap_or(&hex);
                    u32::from_str_radix(hex, 16).ok().map(|rgb| (name, rgb << 8 | 0xff))
                })
                .collect(),
        )
    }
}

/// The one colour, and it is the scheme's own. Used for the logo, the focused
/// workspace and the dials, so that when it appears it means something.
pub fn accent() -> Hsla {
    coloured("primary", 0xff5449ff)
}

/// Amber, and not the scheme's: a warning that turned the scheme's own
/// colour would stop reading as a warning on half of them.
pub fn warn() -> Hsla {
    rgba(0xf5a25dff).into()
}

pub fn alert() -> Hsla {
    coloured("tertiary", 0xff8a80ff)
}

/// The ink on the accent: the focused workspace's number.
pub fn on_accent() -> Hsla {
    coloured("onPrimary", 0x1a0d0cff)
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

/// A panel that hangs off the bar, for one that has to read as the bar
/// opening rather than as a second surface parked under it.
///
/// Two things gave it away. A pane's own gradient starts at the light end,
/// and the bar's has just finished at the dark one, so the shared edge had a
/// step across it in the wrong direction — the panel was lighter than the
/// thing it hangs from. And the hairline a floating pane draws inside its
/// edge was drawn along that edge too, which is a window's outline exactly
/// where there should be no edge at all.
///
/// So this begins where `face` ends, and carries no rim.
pub fn hanging() -> Background {
    // As dense as the bar, and falling off as sharply.
    //
    // A pane is thinner because the compositor blurs what is behind it;
    // nothing blurs behind this, for the same reason nothing blurs behind the
    // bar, so it carries its own body exactly as the pill does.
    //
    // The fall has to happen near the top rather than over the whole height.
    // The bar's glass loses ten levels in its thirty-eight pixels, which is
    // what makes it read as lit from above; the same two colours spread down
    // four hundred and fifty lose five, which reads as one flat colour beside
    // it. So the second stop comes at a third of the way down and the rest
    // holds, and the two surfaces catch the light the same way.
    // Starting a shade under the glass's own end, not at it: the bar darkens
    // its last rows with the shadow it casts inside its edge, so matching the
    // colour `face` ends on leaves the drawer three levels lighter than the
    // row above it — little, and enough to draw a line across a join that is
    // supposed to have none.
    linear_gradient(180., linear_color_stop(rgba(0x0a0a0ce0), 0.), linear_color_stop(rgba(0x050508ea), 0.33))
}

/// Its shadow, and not one pixel of it above the drawer's own top edge.
///
/// This surface is an overlay and the bar is not, so it is drawn over the
/// bar: a shadow that reaches upward is painted onto the thing the drawer is
/// supposed to be part of. A pane's reaches eighteen pixels up, which put a
/// black band along the bottom of the bar wherever the drawer was open — the
/// join it was meant to hide being the one thing it drew attention to.
///
/// Each is offset further than it spreads, so every one of them falls away
/// below. No inset hairline either: see `hanging`.
pub fn hanging_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(26.), hsla(0., 0., 0., 0.5)).blur_radius(px(36.)).spread_radius(px(-14.)),
        BoxShadow::new(px(0.), px(6.), hsla(0., 0., 0., 0.42)).blur_radius(px(12.)).spread_radius(px(-6.)),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_is_the_scheme_s_and_not_the_one_it_was_written_with() {
        // 0xff5449 is what the stylesheet had, and it is also what the
        // scheme happened to be on the machine cae was written on — which is
        // exactly why nobody noticed it was frozen. The test that catches
        // that is the one that reads the file rather than the constant.
        let set: std::collections::HashMap<String, String> =
            cae_core::scheme::colours().into_iter().collect();
        let Some(primary) = set.get("primary") else {
            return; // no scheme on this machine; the fallback is all there is
        };
        let want: u32 = u32::from_str_radix(primary.strip_prefix('#').unwrap_or(primary), 16).unwrap();
        let expected: Hsla = rgba(want << 8 | 0xff).into();
        let got = accent();
        assert!(
            (got.h - expected.h).abs() < 0.001 && (got.s - expected.s).abs() < 0.001,
            "accent is {got:?}, the scheme says {expected:?}"
        );

        // On a machine whose scheme IS the old constant the check above
        // passes either way, which is how this went unnoticed. Asking for
        // the same colour under a fallback it could not have come from
        // proves the file was read.
        let read_not_assumed = coloured("primary", 0x00000000);
        assert_eq!(read_not_assumed, expected, "the scheme file was not read at all");
    }

    #[test]
    fn a_colour_the_scheme_does_not_set_keeps_the_one_it_was_written_with() {
        // Nothing in a Material You scheme is called this, so it can only
        // come back as the fallback.
        let fallback: Hsla = rgba(0x123456ff).into();
        assert_eq!(coloured("nothing-is-called-this", 0x123456ff), fallback);
    }
}
