//! The handful of things the eggs are drawn with.
//!
//! Both scenes were QML: rectangles with rounded corners, a few polylines,
//! some gradients and a soft glow. GPUI draws paths and text and nothing
//! else, so those become paths — and because the whole picture moves under a
//! camera, every one of them goes through the same [`Cam`] on the way.

use gpui::{
    App, Background, Hsla, PathBuilder, PathStyle, Pixels, Point, SharedString, StrokeOptions, TextAlign, TextRun,
    Window, linear_color_stop, linear_gradient, point, px, rgba,
};
use lyon::tessellation::{LineCap, LineJoin};

use crate::theme;

/// A turn and a squash about a point, done to a part of the picture before
/// the camera sees it. The QML had these as `transformOrigin` and a stack of
/// `Rotation`/`Scale` transforms; they nest the same way here.
#[derive(Clone, Copy)]
struct Local {
    about: (f32, f32),
    /// In full turns, not degrees: nothing here is read off a protractor.
    turn: f32,
    across: f32,
    down: f32,
}

impl Local {
    fn put(&self, x: f32, y: f32) -> (f32, f32) {
        let (ax, ay) = self.about;
        let (dx, dy) = ((x - ax) * self.across, (y - ay) * self.down);
        let angle = self.turn * std::f32::consts::TAU;
        let (sin, cos) = (angle.sin(), angle.cos());
        (ax + dx * cos - dy * sin, ay + dx * sin + dy * cos)
    }
}

/// Where the scene is looked at from: the picture is drawn in the screen's
/// own units, then pushed in a little and shaken.
///
/// A camera may also carry a few turns of its own, innermost first, which is
/// how a tank rocks on its tracks while the whole picture leans.
#[derive(Clone, Copy, Default)]
pub struct Cam {
    /// Where the surface begins, which is what everything is relative to.
    pub origin: Point<Pixels>,
    /// The middle of the picture, which is what it is pushed in about.
    pub middle: (f32, f32),
    pub scale: f32,
    pub shift: (f32, f32),
    turns: [Option<Local>; 4],
}

impl Cam {
    pub fn new(origin: Point<Pixels>, middle: (f32, f32), scale: f32, shift: (f32, f32)) -> Cam {
        Cam { origin, middle, scale, shift, turns: [None; 4] }
    }

    /// Straight through, for the things that are not in the world: the
    /// letterbox, the vignettes, the flash.
    pub fn still(origin: Point<Pixels>) -> Cam {
        Cam::new(origin, (0., 0.), 1., (0., 0.))
    }

    /// The same camera with one more turn inside it. A fifth is ignored:
    /// nothing in either scene nests that deep, and silently dropping it is
    /// better than a panic in the middle of a joke.
    pub fn about(&self, about: (f32, f32), turn: f32, across: f32, down: f32) -> Cam {
        let mut turned = *self;
        if let Some(slot) = turned.turns.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(Local { about, turn, across, down });
        }
        turned
    }

    /// The same, turned only.
    pub fn turned(&self, about: (f32, f32), turn: f32) -> Cam {
        self.about(about, turn, 1., 1.)
    }

    pub fn put(&self, x: f32, y: f32) -> Point<Pixels> {
        let (mut x, mut y) = (x, y);
        // Innermost first: the last one added is the one closest to the
        // shape, exactly as the QML transform list read.
        for turn in self.turns.iter().rev().flatten() {
            (x, y) = turn.put(x, y);
        }
        let (mx, my) = self.middle;
        point(
            self.origin.x + px((x - mx) * self.scale + mx + self.shift.0),
            self.origin.y + px((y - my) * self.scale + my + self.shift.1),
        )
    }

    /// A length as it comes out the other side. Stroke widths, mostly, which
    /// have no direction to be squashed along.
    pub fn span(&self, length: f32) -> f32 {
        let squash: f32 = self.turns.iter().flatten().map(|turn| (turn.across.abs() + turn.down.abs()) / 2.).product();
        length * self.scale * squash
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

/// A rectangle with a line round it rather than a colour in it.
pub fn rect_edge(window: &mut Window, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), radius: f32, width: f32, colour: Hsla) {
    if w <= 0. || h <= 0. || width <= 0. {
        return;
    }
    let width = px(cam.span(width));
    let mut path = PathBuilder::stroke(width).with_style(PathStyle::Stroke(
        StrokeOptions::default().with_line_width(f32::from(width)).with_line_join(LineJoin::Round),
    ));
    trace_rect(&mut path, cam, x, y, w, h, radius);
    if let Ok(built) = path.build() {
        window.paint_path(built, colour);
    }
}

/// Top to bottom, which is the only way any of these gradients run.
pub fn down(top: Hsla, bottom: Hsla) -> Background {
    linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bottom, 1.))
}

/// A round shape. `squash` under one makes it an oval.
pub fn disc(window: &mut Window, cam: &Cam, x: f32, y: f32, radius: f32, squash: f32, fill: impl Into<Background>) {
    rect(window, cam, (x - radius, y - radius * squash, radius * 2., radius * 2. * squash), radius.max(1.), fill);
}

/// The soft aura everything in the cinema sits in. A real radial gradient is
/// not on offer, so it is rings: enough of them that the steps do not show,
/// each one fainter than the last.
pub fn glow(window: &mut Window, cam: &Cam, x: f32, y: f32, radius: f32, colour: u32, strength: f32) {
    const RINGS: usize = 9;
    for ring in (0..RINGS).rev() {
        let out = (ring + 1) as f32 / RINGS as f32;
        // What the QML gradient said: full strength in the middle, a third
        // of it by just past halfway, nothing at the rim.
        let alpha = if out <= 0.55 {
            strength * (1. - out / 0.55 * 0.65)
        } else {
            strength * 0.35 * (1. - (out - 0.55) / 0.45)
        };
        disc(window, cam, x, y, radius * out, 1., shade(colour, alpha / RINGS as f32 * 2.2));
    }
}

/// A run of straight lines, drawn with a pen.
pub fn line(window: &mut Window, cam: &Cam, points: &[(f32, f32)], width: f32, colour: Hsla) {
    let Some((first, rest)) = points.split_first() else { return };
    let width = px(cam.span(width));
    let mut path = PathBuilder::stroke(width).with_style(PathStyle::Stroke(
        StrokeOptions::default().with_line_width(f32::from(width)).with_line_join(LineJoin::Round).with_line_cap(LineCap::Round),
    ));
    path.move_to(cam.put(first.0, first.1));
    for step in rest {
        path.line_to(cam.put(step.0, step.1));
    }
    if let Ok(built) = path.build() {
        window.paint_path(built, colour);
    }
}

/// The same run, with a colour inside it.
pub fn shape(window: &mut Window, cam: &Cam, points: &[(f32, f32)], fill: Hsla, edge: Option<(f32, Hsla)>) {
    let Some((first, rest)) = points.split_first() else { return };
    let mut path = PathBuilder::fill();
    path.move_to(cam.put(first.0, first.1));
    for step in rest {
        path.line_to(cam.put(step.0, step.1));
    }
    path.close();
    if let Ok(built) = path.build() {
        window.paint_path(built, fill);
    }
    if let Some((width, colour)) = edge {
        line(window, cam, points, width, colour);
    }
}

/// A word. The only text either scene has, and there are three of them.
pub fn word(
    window: &mut Window,
    cx: &mut App,
    cam: &Cam,
    text: impl Into<SharedString>,
    (x, y): (f32, f32),
    size: f32,
    colour: Hsla,
    centred: bool,
) {
    let size = px(cam.span(size));
    if size <= px(1.) {
        return;
    }
    let text: SharedString = text.into();
    let run = TextRun {
        len: text.len(),
        font: gpui::Font {
            family: theme::FONT.into(),
            features: Default::default(),
            fallbacks: None,
            weight: gpui::FontWeight::BOLD,
            style: Default::default(),
        },
        color: colour,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(text, size, &[run], None);
    let at = cam.put(x, y);
    let at = if centred { point(at.x - shaped.width / 2., at.y) } else { at };
    let _ = shaped.paint(at, size * 1.2, TextAlign::Left, None, window, cx);
}
