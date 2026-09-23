//! The spectrum, as a texture across the pill rather than a widget in a slot.
//!
//! It sits behind everything the bar draws: loud enough to notice moving,
//! quiet enough that the clock on top of it stays legible. It is a view of its
//! own so that its fifteen frames a second are a repaint of a few hundred
//! rectangles and nothing else.

use gpui::{App, Bounds, Context, IntoElement, Pixels, Render, Size, Styled, Window, canvas, fill, point, px};

use crate::feeds::{Feed, Feeds, Spectrum};
use crate::theme;
use crate::ui::{lock, rsx};

/// Thin bars with air between them, repeated across the whole width. A band
/// wide enough to read individually stops being a spectrum and starts being a
/// bar chart.
const BAND: f32 = 2.;
const GAP: f32 = 3.;

pub struct Visualiser {
    spectrum: gpui::Entity<Feed<Spectrum>>,
    output: String,
    /// Whether a fullscreen window has this bar's screen, and the bar with
    /// it: Hyprland goes on asking a hidden bar for frames, and every frame
    /// drawn there is drawn for nobody.
    covered: bool,
}

impl Visualiser {
    pub fn new(output: String, feeds: &Feeds, cx: &mut Context<Self>) -> Visualiser {
        cx.observe(&feeds.spectrum, |visualiser: &mut Visualiser, _, cx| {
            if !visualiser.covered {
                cx.notify();
            }
        })
        .detach();
        cx.observe(&feeds.hypr, |visualiser: &mut Visualiser, hypr, cx| {
            let covered = hypr.read(cx).value.fullscreen.contains(&visualiser.output);
            if covered != visualiser.covered {
                visualiser.covered = covered;
                cx.notify();
            }
        })
        .detach();
        let covered = feeds.hypr.read(cx).value.fullscreen.contains(&output);
        Visualiser { spectrum: feeds.spectrum.clone(), output, covered }
    }
}

/// Keeps the spectrum recorded only while some bar that shows it can be
/// seen: the entry is on, the session is not locked, and not every screen
/// has a fullscreen window over its bar.
pub fn keep_wanted(cx: &mut App, feeds: &Feeds) {
    fn follow(feeds: &Feeds, cx: &mut App) {
        let shown = feeds.settings.read(cx).value.layout.entries.iter().any(|entry| entry == "visualiser");
        let hypr = &feeds.hypr.read(cx).value;
        // No names at all is a compositor that does not say, which is no
        // reason to think the bar hidden.
        let in_view = hypr.outputs.is_empty() || hypr.outputs.iter().any(|output| !hypr.fullscreen.contains(output));
        feeds.spectrum_wanted.set(shown && in_view && !lock::is_locked(cx));
    }

    follow(feeds, cx);
    let watched = feeds.clone();
    cx.observe(&feeds.settings, move |_, cx| follow(&watched, cx)).detach();
    let watched = feeds.clone();
    cx.observe(&feeds.hypr, move |_, cx| follow(&watched, cx)).detach();
    let watched = feeds.clone();
    lock::observe(cx, move |cx| follow(&watched, cx));
}

impl Render for Visualiser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.spectrum.read(cx).value.clone();
        rsx! {
            <canvas
                class="size-full"
                prepaint={|_, _, _| ()}
                paint={move |bounds, _, window, _| paint(&frame, bounds, window)}
            />
        }
    }
}

fn paint(frame: &Spectrum, bounds: Bounds<Pixels>, window: &mut Window) {
    if !frame.live || frame.bars.is_empty() {
        return;
    }
    // One layer for the lot. GPUI works out a drawing order for every
    // primitive it is given by searching a tree of everything painted so far,
    // and a few hundred bands a frame made that search a third of what this
    // whole process spent. Inside a layer nothing is searched: its contents
    // are promised not to overlap, which bands standing side by side do not.
    window.paint_layer(bounds, |window| bands(frame, bounds, window));
}

fn bands(frame: &Spectrum, bounds: Bounds<Pixels>, window: &mut Window) {
    let height = f32::from(bounds.size.height);
    let width = f32::from(bounds.size.width);
    let radius = height / 2.;
    let pitch = BAND + GAP;
    let cycle = frame.bars.len() * 2;

    for index in 0..(width / pitch) as usize {
        // Walk the spectrum out and back, so bass meets bass where the
        // pattern repeats rather than cutting from treble to bass.
        let step = index % cycle;
        let level = frame.bars[if step < frame.bars.len() { step } else { cycle - 1 - step }];
        let tall = (f32::from(level) / 255. * height * 0.75).max(1.);

        // The pill's ends are round and a rectangle is not. Where the pill
        // curves away, the band stands on the curve and stops at it, which is
        // what clipping to the pill's shape did when a stylesheet could.
        let x = index as f32 * pitch;
        let into_corner = (radius - x).max(x + BAND - (width - radius)).max(0.);
        let lift = radius - (radius * radius - into_corner * into_corner).max(0.).sqrt();
        let tall = tall.min(height - 2. * lift);
        if tall <= 0. {
            continue;
        }

        let band = Bounds::new(
            point(bounds.origin.x + px(x), bounds.origin.y + px(height - lift - tall)),
            Size::new(px(BAND), px(tall)),
        );
        window.paint_quad(fill(band, theme::white(0.14)));
    }
}
