//! A reading as an arc: how much of something, out of all there is of it.
//!
//! The bar's three dials and the dashboard's larger ones are the same
//! drawing at different sizes: a faint track, and over it as much of the
//! track as the reading is. It fills clockwise, which is the only direction
//! anybody reads one.

use gpui::{Bounds, Hsla, IntoElement, PathBuilder, PathStyle, Pixels, StrokeOptions, Styled, Window, canvas, point, px};
use lyon::tessellation::LineCap;

use crate::theme;
use crate::ui::rsx;

#[derive(Clone, Copy)]
pub struct Dial {
    pub size: Pixels,
    pub stroke: Pixels,
    /// How much of a circle the track is: all of it for a ring, three
    /// quarters for a gauge with its opening at the foot.
    pub sweep: f32,
    pub colour: Hsla,
}

impl Dial {
    /// A closed ring, in the accent.
    pub fn ring(size: Pixels, stroke: Pixels) -> Dial {
        Dial { size, stroke, sweep: 1., colour: theme::accent() }
    }

    /// Open at the foot, where a gauge has room for what it is a gauge of.
    pub fn gauge(size: Pixels, stroke: Pixels) -> Dial {
        Dial { sweep: 0.75, ..Dial::ring(size, stroke) }
    }

    pub fn coloured(self, colour: Hsla) -> Dial {
        Dial { colour, ..self }
    }
}

pub fn dial(percent: f64, look: Dial) -> impl IntoElement {
    let part = (percent.clamp(0., 100.) / 100.) as f32;
    rsx! {
        <canvas
            class="flex-none"
            size={look.size}
            prepaint={|_, _, _| ()}
            paint={move |bounds, _, window, _| paint(bounds, part, look, window)}
        />
    }
}

fn paint(bounds: Bounds<Pixels>, part: f32, look: Dial, window: &mut Window) {
    let centre = bounds.center();
    let radius = (look.size - look.stroke) / 2. - px(1.);
    // Turns clockwise from twelve o'clock. A track that is less than a
    // circle is centred on the top, so that what is missing is at the foot.
    let begins = 0.5 + (1. - look.sweep) / 2.;
    let at = |turn: f32| {
        let angle = (turn - 0.25) * std::f32::consts::TAU;
        point(centre.x + radius * angle.cos(), centre.y + radius * angle.sin())
    };
    let arc = |length: f32| {
        // Round ends, which is what makes a short reading a dot rather than
        // a sliver.
        let round = StrokeOptions::default().with_line_width(f32::from(look.stroke)).with_line_cap(LineCap::Round);
        let mut path = PathBuilder::stroke(look.stroke).with_style(PathStyle::Stroke(round));
        path.move_to(at(begins));
        // In pieces of under half a turn: one call cannot say which way
        // round a longer arc it means.
        let pieces = (length / 0.3).ceil().max(1.) as usize;
        for piece in 1..=pieces {
            path.arc_to(point(radius, radius), px(0.), false, true, at(begins + length * piece as f32 / pieces as f32));
        }
        path.build()
    };

    if let Ok(track) = arc(look.sweep.min(0.9999)) {
        window.paint_path(track, theme::white(0.1));
    }
    if part > 0.004
        && let Ok(reading) = arc((look.sweep * part).min(0.9999))
    {
        window.paint_path(reading, look.colour);
    }
}
