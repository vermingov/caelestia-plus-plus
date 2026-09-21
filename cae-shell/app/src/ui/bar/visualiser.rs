//! The spectrum, as a texture across the pill rather than a widget in a slot.
//!
//! It sits behind everything the bar draws: loud enough to notice moving,
//! quiet enough that the clock on top of it stays legible. It is a view of its
//! own so that its fifteen frames a second are a repaint of a few hundred
//! rectangles and nothing else.

use gpui::{Bounds, Context, IntoElement, Pixels, Render, Size, Styled, Window, canvas, fill, point, px};

use crate::feeds::{Feed, Feeds, Spectrum};
use crate::theme;
use crate::ui::rsx;

/// Thin bars with air between them, repeated across the whole width. A band
/// wide enough to read individually stops being a spectrum and starts being a
/// bar chart.
const BAND: f32 = 2.;
const GAP: f32 = 3.;

pub struct Visualiser {
    spectrum: gpui::Entity<Feed<Spectrum>>,
}

impl Visualiser {
    pub fn new(feeds: &Feeds, cx: &mut Context<Self>) -> Visualiser {
        cx.observe(&feeds.spectrum, |_, _, cx| cx.notify()).detach();
        Visualiser { spectrum: feeds.spectrum.clone() }
    }
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
