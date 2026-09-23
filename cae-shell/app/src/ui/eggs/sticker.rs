//! The two drawings the desktop egg is made of, as the shapes they were
//! drawn as.
//!
//! They were a pair of SVGs the QML shell handed to Qt. GPUI will draw an
//! SVG, but only in one colour, and only without turning it — and the whole
//! scene is one thing leaning into another. So the shapes are here instead,
//! in the same order and the same colours their files had, and the turn and
//! the size are done to the points on the way past.

use gpui::{Hsla, PathBuilder, PathStyle, Pixels, Point, StrokeOptions, Window, px, rgba};

use super::paint::Cam;
use lyon::tessellation::{LineCap, LineJoin};

/// One step of an outline, in the drawing's own units.
#[derive(Clone, Copy)]
pub enum Step {
    Start(f32, f32),
    /// A quadratic: the control point, then where it ends. Both where they
    /// are in the drawing, never where they are from the last point.
    Curve(f32, f32, f32, f32),
    /// A cubic: two control points, then where it ends.
    Bend(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Clone, Copy)]
pub enum Shape {
    Round { x: f32, y: f32, radius: f32 },
    /// A round shape squashed on one axis, turned by `turn` full turns.
    Oval { x: f32, y: f32, across: f32, down: f32, turn: f32 },
    Line { from: (f32, f32), to: (f32, f32) },
    Outline(&'static [Step]),
}

/// A shape and how it is painted: the colour inside it, the colour round it,
/// and how much of either shows.
#[derive(Clone, Copy)]
pub struct Part {
    pub shape: Shape,
    pub fill: Option<u32>,
    pub stroke: Option<(u32, f32)>,
    pub alpha: f32,
}

const fn part(shape: Shape, fill: u32, stroke: u32, width: f32) -> Part {
    Part { shape, fill: Some(fill), stroke: Some((stroke, width)), alpha: 1. }
}

const fn tint(shape: Shape, fill: u32, alpha: f32) -> Part {
    Part { shape, fill: Some(fill), stroke: None, alpha }
}

const fn line(shape: Shape, stroke: u32, width: f32, alpha: f32) -> Part {
    Part { shape, fill: None, stroke: Some((stroke, width)), alpha }
}

/// A whole drawing, and the box it was drawn in.
pub struct Sticker {
    pub across: f32,
    pub down: f32,
    pub parts: &'static [Part],
}

const INK: u32 = 0x7a4a2b;
const VEIN: u32 = 0xcf8f83;
const SHINE: u32 = 0xffffff;

/// The one that rises first.
pub const RISER: Sticker = Sticker {
    across: 220.,
    down: 270.,
    parts: &[
        part(Shape::Round { x: 72., y: 216., radius: 46. }, 0xf2bf94, INK, 7.),
        part(Shape::Round { x: 148., y: 216., radius: 46. }, 0xf6c9a3, INK, 7.),
        part(
            Shape::Outline(&[
                Step::Start(76., 210.),
                Step::Curve(70., 110., 84., 58.),
                Step::Curve(92., 26., 110., 26.),
                Step::Curve(128., 26., 136., 58.),
                Step::Curve(150., 110., 144., 210.),
                Step::Curve(110., 232., 76., 210.),
                Step::Close,
            ]),
            0xf6c9a3,
            INK,
            7.,
        ),
        part(
            Shape::Outline(&[
                Step::Start(84., 62.),
                Step::Curve(92., 30., 110., 30.),
                Step::Curve(128., 30., 136., 62.),
                Step::Curve(140., 78., 137., 86.),
                Step::Curve(124., 100., 96., 100.),
                Step::Curve(86., 92., 84., 78.),
                Step::Close,
            ]),
            0xe8a179,
            INK,
            7.,
        ),
        line(Shape::Line { from: (110., 38.), to: (110., 52.) }, INK, 6., 1.),
        line(
            Shape::Outline(&[Step::Start(92., 120.), Step::Curve(102., 132., 94., 146.), Step::Curve(86., 160., 98., 172.)]),
            VEIN,
            5.,
            0.8,
        ),
        line(
            Shape::Outline(&[
                Step::Start(124., 105.),
                Step::Curve(114., 121., 123., 135.),
                Step::Curve(131., 148., 121., 163.),
                Step::Curve(113., 175., 123., 187.),
            ]),
            VEIN,
            5.,
            0.8,
        ),
        line(Shape::Outline(&[Step::Start(104., 165.), Step::Curve(116., 175., 112., 191.)]), VEIN, 5., 0.8),
        tint(Shape::Oval { x: 97., y: 52., across: 7., down: 12., turn: -14. / 360. }, SHINE, 0.45),
        tint(Shape::Oval { x: 55., y: 200., across: 10., down: 14., turn: -20. / 360. }, SHINE, 0.35),
    ],
};

/// The one that joins it.
pub const PARTNER: Sticker = Sticker {
    across: 240.,
    down: 200.,
    parts: &[
        part(
            Shape::Outline(&[
                Step::Start(120., 18.),
                Step::Bend(62., 18., 32., 78., 32., 122.),
                Step::Bend(32., 170., 76., 188., 120., 188.),
                Step::Bend(164., 188., 208., 170., 208., 122.),
                Step::Bend(208., 78., 178., 18., 120., 18.),
                Step::Close,
            ]),
            0xf6c9a3,
            INK,
            7.,
        ),
        part(
            Shape::Outline(&[
                Step::Start(120., 64.),
                Step::Bend(106., 96., 106., 138., 120., 166.),
                Step::Bend(134., 138., 134., 96., 120., 64.),
                Step::Close,
            ]),
            0xe88ba0,
            INK,
            7.,
        ),
        part(Shape::Round { x: 120., y: 58., radius: 9. }, 0xe88ba0, INK, 6.),
        tint(Shape::Oval { x: 70., y: 120., across: 12., down: 8., turn: 0. }, 0xf0a8a0, 0.6),
        tint(Shape::Oval { x: 170., y: 120., across: 12., down: 8., turn: 0. }, 0xf0a8a0, 0.6),
        tint(Shape::Oval { x: 66., y: 60., across: 9., down: 14., turn: 18. / 360. }, SHINE, 0.4),
    ],
};

/// Where a drawing is, how big, and which way up.
#[derive(Clone, Copy)]
pub struct Placed {
    /// Where the drawing's own origin lands on the screen.
    pub at: Point<Pixels>,
    /// What it is turned about, in the drawing's own units. Both stickers
    /// turn about the foot, as the QML ones did.
    pub about: (f32, f32),
    pub turn: f32,
    pub scale: f32,
    /// How much of it shows, over whatever each part asks for.
    pub alpha: f32,
}

/// How round a shape is made of four bends. The number every drawing program
/// uses for the same job.
const ROUNDNESS: f32 = 0.552_284_75;

/// The dark it sits in, under everything else it is made of.
///
/// Without one it is a drawing laid on the screen; with one it is standing on
/// the bottom of it. Nine ovals rather than a blur, each wider and fainter
/// than the last, which is the same trick `glow` uses and the only one on
/// offer without a real shadow to cast.
fn footing(sticker: &Sticker, placed: Placed, cam: &Cam, window: &mut Window) {
    const RINGS: usize = 9;
    let width = sticker.across * placed.scale * 0.42;
    let (x, y) = (f32::from(placed.at.x), f32::from(placed.at.y) + sticker.down * placed.scale * 0.5);
    for ring in (0..RINGS).rev() {
        let out = (ring + 1) as f32 / RINGS as f32;
        let alpha = 0.22 * (1. - out) * placed.alpha;
        crate::ui::eggs::paint::disc(window, cam, x, y, width * (0.55 + out * 0.9), 0.16,
            crate::ui::eggs::paint::shade(0x000000, alpha));
    }
}

pub fn paint(sticker: &Sticker, placed: Placed, cam: &Cam, window: &mut Window) {
    footing(sticker, placed, cam, window);
    let (angle, (about_x, about_y)) = (placed.turn * std::f32::consts::TAU, placed.about);
    let (sin, cos) = (angle.sin(), angle.cos());
    let put = move |x: f32, y: f32| {
        let (dx, dy) = ((x - about_x) * placed.scale, (y - about_y) * placed.scale);
        cam.put(f32::from(placed.at.x) + dx * cos - dy * sin, f32::from(placed.at.y) + dx * sin + dy * cos)
    };

    for piece in sticker.parts {
        if let Some(colour) = piece.fill {
            let mut path = PathBuilder::fill();
            trace(&mut path, piece.shape, &put);
            if let Ok(built) = path.build() {
                window.paint_path(built, crate::ui::eggs::paint::modelled(colour, piece.alpha * placed.alpha));
            }
        }
        if let Some((colour, width)) = piece.stroke {
            let width = px(cam.span(width * placed.scale));
            let round = StrokeOptions::default()
                .with_line_width(f32::from(width))
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round);
            let mut path = PathBuilder::stroke(width).with_style(PathStyle::Stroke(round));
            trace(&mut path, piece.shape, &put);
            if let Ok(built) = path.build() {
                window.paint_path(built, shade(colour, piece.alpha * placed.alpha));
            }
        }
    }
}

fn shade(colour: u32, alpha: f32) -> Hsla {
    rgba((colour << 8) | (alpha.clamp(0., 1.) * 255.) as u32).into()
}

fn trace(path: &mut PathBuilder, shape: Shape, put: &impl Fn(f32, f32) -> Point<Pixels>) {
    match shape {
        Shape::Round { x, y, radius } => oval(path, x, y, radius, radius, 0., put),
        Shape::Oval { x, y, across, down, turn } => oval(path, x, y, across, down, turn, put),
        Shape::Line { from, to } => {
            path.move_to(put(from.0, from.1));
            path.line_to(put(to.0, to.1));
        }
        Shape::Outline(steps) => {
            for step in steps {
                match *step {
                    Step::Start(x, y) => path.move_to(put(x, y)),
                    Step::Curve(cx, cy, x, y) => path.curve_to(put(x, y), put(cx, cy)),
                    Step::Bend(ax, ay, bx, by, x, y) => path.cubic_bezier_to(put(x, y), put(ax, ay), put(bx, by)),
                    Step::Close => path.close(),
                }
            }
        }
    }
}

/// A round or squashed shape, as four bends. `turn` is in full turns, about
/// the shape's own middle. Also what the scene's droplets are.
pub fn oval(path: &mut PathBuilder, x: f32, y: f32, across: f32, down: f32, turn: f32, put: &impl Fn(f32, f32) -> Point<Pixels>) {
    let angle = turn * std::f32::consts::TAU;
    let (sin, cos) = (angle.sin(), angle.cos());
    let on = |dx: f32, dy: f32| put(x + dx * cos - dy * sin, y + dx * sin + dy * cos);
    let (rx, ry) = (across * ROUNDNESS, down * ROUNDNESS);

    path.move_to(on(0., -down));
    path.cubic_bezier_to(on(across, 0.), on(rx, -down), on(across, -ry));
    path.cubic_bezier_to(on(0., down), on(across, ry), on(rx, down));
    path.cubic_bezier_to(on(-across, 0.), on(-rx, down), on(-across, ry));
    path.cubic_bezier_to(on(0., -down), on(-across, -ry), on(-rx, -down));
    path.close();
}
