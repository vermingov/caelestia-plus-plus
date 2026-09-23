//! The handful of things the desktop egg is drawn with.
//!
//! It was QML: rectangles with rounded corners and some gradients. GPUI
//! draws paths and text and nothing else, so those become paths — and
//! because the whole picture moves under a camera, every one of them goes
//! through the same [`Cam`] on the way.

use gpui::{Background, Hsla, PathBuilder, Pixels, Point, Window, linear_color_stop, linear_gradient, point, px, rgba};

/// Where the scene is looked at from: the picture is drawn in the screen's
/// own units, then pushed in a little and shaken.
#[derive(Clone, Copy, Default)]
pub struct Cam {
    /// Where the surface begins, which is what everything is relative to.
    pub origin: Point<Pixels>,
    /// The middle of the picture, which is what it is pushed in about.
    pub middle: (f32, f32),
    pub scale: f32,
    pub shift: (f32, f32),
}

impl Cam {
    pub fn new(origin: Point<Pixels>, middle: (f32, f32), scale: f32, shift: (f32, f32)) -> Cam {
        Cam { origin, middle, scale, shift }
    }

    /// Straight through, for the things that are not in the world: the
    /// letterbox, the vignettes, the flash.
    pub fn still(origin: Point<Pixels>) -> Cam {
        Cam::new(origin, (0., 0.), 1., (0., 0.))
    }

    pub fn put(&self, x: f32, y: f32) -> Point<Pixels> {
        let (mx, my) = self.middle;
        point(
            self.origin.x + px((x - mx) * self.scale + mx + self.shift.0),
            self.origin.y + px((y - my) * self.scale + my + self.shift.1),
        )
    }

    /// A length as it comes out the other side. Stroke widths, mostly, which
    /// have no direction to be squashed along.
    pub fn span(&self, length: f32) -> f32 {
        length * self.scale
    }
}

pub fn shade(colour: u32, alpha: f32) -> Hsla {
    rgba((colour << 8) | (alpha.clamp(0., 1.) * 255.) as u32).into()
}

/// The same colour, moved toward white or black. `by` is how far, one being
/// all the way.
pub fn lit(colour: u32, by: f32) -> u32 {
    let mix = |channel: u32| {
        let channel = channel as f32;
        let moved = if by >= 0. { channel + (255. - channel) * by } else { channel * (1. + by) };
        (moved.clamp(0., 255.)) as u32
    };
    mix(colour >> 16 & 0xff) << 16 | mix(colour >> 8 & 0xff) << 8 | mix(colour & 0xff)
}

/// A flat colour given form: lighter where the light would fall, darker
/// underneath.
///
/// The drawings came across from the files as flat fills, one colour to a
/// shape, which is what an SVG is and what the old shell drew. A shape with
/// one colour in it reads as a sticker of the thing rather than the thing,
/// and no amount of moving it about fixes that. Every fill is a short
/// gradient now, which costs nothing here — the same path, one more stop —
/// and is the whole difference between a cut-out and something with a side
/// to it.
pub fn modelled(colour: u32, alpha: f32) -> Background {
    // Gently. Far enough to see a side to the shape, not so far that the
    // colour turns to dirt at the bottom of it.
    down(shade(lit(colour, 0.17), alpha), shade(lit(colour, -0.12), alpha))
}

/// A rectangle, with corners as round as `radius`.
fn trace_rect(path: &mut PathBuilder, cam: &Cam, x: f32, y: f32, w: f32, h: f32, radius: f32) {
    let radius = radius.min(w / 2.).min(h / 2.).max(0.);
    if radius <= 0.1 {
        path.move_to(cam.put(x, y));
        path.line_to(cam.put(x + w, y));
        path.line_to(cam.put(x + w, y + h));
        path.line_to(cam.put(x, y + h));
        path.close();
        return;
    }
    // A quarter turn is one bend, with the control points where every
    // drawing program puts them.
    const ROUNDNESS: f32 = 0.552_284_75;
    let pull = radius * ROUNDNESS;
    let (r, b) = (x + w, y + h);
    path.move_to(cam.put(x + radius, y));
    path.line_to(cam.put(r - radius, y));
    path.cubic_bezier_to(cam.put(r, y + radius), cam.put(r - radius + pull, y), cam.put(r, y + radius - pull));
    path.line_to(cam.put(r, b - radius));
    path.cubic_bezier_to(cam.put(r - radius, b), cam.put(r, b - radius + pull), cam.put(r - radius + pull, b));
    path.line_to(cam.put(x + radius, b));
    path.cubic_bezier_to(cam.put(x, b - radius), cam.put(x + radius - pull, b), cam.put(x, b - radius + pull));
    path.line_to(cam.put(x, y + radius));
    path.cubic_bezier_to(cam.put(x + radius, y), cam.put(x, y + radius - pull), cam.put(x + radius - pull, y));
    path.close();
}

pub fn rect(window: &mut Window, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), radius: f32, fill: impl Into<Background>) {
    if w <= 0. || h <= 0. {
        return;
    }
    let mut path = PathBuilder::fill();
    trace_rect(&mut path, cam, x, y, w, h, radius);
    if let Ok(built) = path.build() {
        window.paint_path(built, fill);
    }
}

/// Top to bottom, which is the way most of these gradients run.
pub fn down(top: Hsla, bottom: Hsla) -> Background {
    linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bottom, 1.))
}

/// A round shape. `squash` under one makes it an oval.
pub fn disc(window: &mut Window, cam: &Cam, x: f32, y: f32, radius: f32, squash: f32, fill: impl Into<Background>) {
    rect(window, cam, (x - radius, y - radius * squash, radius * 2., radius * 2. * squash), radius.max(1.), fill);
}
